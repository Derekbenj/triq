// The page: it asks twice a second which programs there are and which of them run, draws that,
// and sends what is clicked. It starts programs and stops them, and no more: what is inside a
// runtime is the workbench's to show.
import { dissolve, mark } from '/hero.js';
import { age, another, clash, doing, earlier, families, family, free, named, others, picture, running } from '/logic.js';

const token = document.querySelector('meta[name=triq-token]').content;
const $ = (id) => document.getElementById(id);

// What the server last said, and what the person has done on the page.
let state = { programs: { found: [], started: [] }, runtimes: [], said: [0, null] };
const view = {
  search: '',
  // What a program is to be told, as typed in its box, and the programs whose box is open.
  told: {},
  asked: new Set(),
  // The cards of those that were started, by their numbers; whose output is open; and those
  // whose output was opened for them once, because they failed.
  cards: new Map(),
  open: new Set(),
  opened: new Set(),
  // What the list of programs and the runtimes of elsewhere were last drawn from, so that
  // they are drawn again only when that changes; and how many problems were said.
  drawn: '',
  elsewhere: '',
  shown: 0,
  // How many times in a row the server did not answer; and whether it is taken to have ended.
  missed: 0,
  gone: false,
};

async function api(path, body) {
  const sent = { method: body === undefined ? 'GET' : 'POST', headers: { 'x-triq-token': token } };
  if (body !== undefined) sent.body = JSON.stringify(body);
  const answer = await fetch(path, sent);
  if (!answer.ok) throw Object.assign(new Error(await answer.text()), { status: answer.status });
  return answer.json();
}

// Say something at the foot of the window: for a while, or until something else is said.
let hush;
function say(words, stays = false) {
  $('said').textContent = words;
  $('said').hidden = false;
  clearTimeout(hush);
  if (!stays) hush = setTimeout(() => ($('said').hidden = true), 8000);
}

// The server has ended, or answers again: the page says so, and is not to be clicked meanwhile.
function ended(gone) {
  if (gone === view.gone) return;
  view.gone = gone;
  document.body.classList.toggle('gone', gone);
  if (gone) say('triq serve has ended, and the programs it started with it. Run triq serve again, and this page comes back.', true);
  else $('said').hidden = true;
  draw();
}

// Send something, say so if it could not be done, and look again at once.
async function send(path, body) {
  try {
    await api(path, body);
  } catch (problem) {
    say(problem.message);
  }
  refresh();
}
const start = (program, told) => send('/api/start', { program, told });

// An element: `h('button.pill', { onclick }, 'Start')`.
function h(what, attributes = {}, ...children) {
  const [tag, ...classes] = what.split('.');
  const element = document.createElement(tag || 'div');
  if (classes.length) element.className = classes.join(' ');
  for (const [name, value] of Object.entries(attributes)) {
    if (name.startsWith('on')) element.addEventListener(name.slice(2), value);
    else if (value !== false && value !== null && value !== undefined) element.setAttribute(name, value === true ? '' : value);
  }
  for (const child of children.flat(Infinity)) {
    if (child !== null && child !== undefined && child !== false) element.append(child);
  }
  return element;
}

// Each family of programs has a colour, and a program's picture is drawn in it.
const COLOURS = { '': '#e0389a', algorithm: '#1f8fff', peripheral: '#f2a900', physics: '#22c55e', simulator: '#ff7a1a', stack: '#8b3dff' };
const SPARE = ['#00b3c7', '#e0389a', '#8b3dff', '#ff7a1a', '#1f8fff'];
const colourOf = (name) => {
  const letters = [...family(name)].reduce((sum, letter) => sum + letter.codePointAt(0), 0);
  return COLOURS[family(name)] ?? SPARE[letters % SPARE.length];
};
const pictured = (name) => {
  const squares = picture(name).flat().map((on) => h(on ? 'i.on' : 'i'));
  return h('span.picture', { style: `--colour: ${colourOf(name)}`, 'aria-hidden': 'true' }, squares);
};
// A program's name without its family, which is said beside it.
const short = (name) => (family(name) ? name.slice(family(name).length + 1) : name);

// A program that can be started. With others of it running, one more is told what the last
// was, under a name that is free: that is said on the card before it is done.
function programCard(name) {
  const { started } = state.programs;
  const runs = running(started, name).length;
  const proposed = another(name, started, state.runtimes);
  const asked = view.asked.has(name);
  const go = () => start(name, asked ? (view.told[name] ?? '') : proposed);
  const ask = () => {
    if (asked) view.asked.delete(name);
    else view.asked.add(name);
    if (!asked && !view.told[name]) view.told[name] = proposed;
    drawPrograms(true);
    document.querySelector(`[data-told="${name}"]`)?.focus();
  };
  const box = h('input', { type: 'text', 'data-told': name, value: view.told[name] ?? '', placeholder: 'as after its name in a terminal: a name, --speed 5', spellcheck: 'false', autocomplete: 'off' });
  box.addEventListener('input', () => (view.told[name] = box.value));
  box.addEventListener('keydown', (key) => key.key === 'Enter' && go());
  const again = (told) => h('button.told', { title: 'It was started with these before', onclick: () => ((view.told[name] = told), drawPrograms(true)) }, told);
  return h(
    'div.card',
    {},
    h(
      'div.row',
      {},
      pictured(name),
      h('div.grow', {}, h('div.name', {}, short(name)), h('div.sub', {}, runs ? h('span.badge', {}, `${runs} running`) : `triq ${name}`)),
      h('button.pill', { onclick: go, title: `triq ${name}` }, runs ? 'Start another' : 'Start'),
    ),
    h(
      'div.under',
      {},
      h('button.link', { onclick: ask }, asked ? 'without arguments of my own' : 'with arguments'),
      !asked && proposed && h('span', {}, 'the next is started with ', h('code', {}, proposed)),
    ),
    asked && h('div.tell', {}, box),
    asked && earlier(started, name).length > 0 && h('div.before', {}, earlier(started, name).map(again)),
  );
}

// The programs, family by family. They are drawn again when something they show has changed,
// and not while something is being typed in one of their boxes.
function drawPrograms(now = false) {
  const { found, started } = state.programs;
  const listed = families(found, view.search);
  const shows = found.map((name) => [running(started, name).length, another(name, started, state.runtimes), earlier(started, name)]);
  const drawn = JSON.stringify([listed, shows, [...view.asked]]);
  if (!now && (drawn === view.drawn || $('programs').contains(document.activeElement))) return;
  view.drawn = drawn;
  const none = found.length
    ? 'No program has that in its name.'
    : 'No program of triq was found. They are looked for beside the triq tool, in the folders registered with triq register <folder>, and on the PATH.';
  const parts = listed.map(({ family: of, programs }) =>
    h('div.family', {}, h('h3', { style: `--colour: ${colourOf(programs[0])}` }, of || 'triq'), h('div.cards', {}, programs.map(programCard))),
  );
  $('programs').replaceChildren(...(parts.length ? parts : [h('p.none', {}, none)]));
}

// The card of one that was started. It is made once and kept, so that its output stays where
// it was scrolled to; `update` brings it in line with what the server says of it.
function startedCard(id) {
  const [status, last, log] = [h('div.state'), h('span'), h('pre.log', { hidden: true })];
  const [name, told, ends] = [h('div.name'), h('div.sub'), h('div.row')];
  const output = h('button.link', {
    onclick: () => {
      if (view.open.has(id)) view.open.delete(id);
      else view.open.add(id);
      draw();
    },
  });
  const face = h('span');
  const root = h('div.card', {}, h('div.row', {}, face, h('div.grow', {}, name, told), ends), status, h('div.last', {}, last, output), log);
  let buttons = '';
  const update = (one) => {
    const how = one.failed ? 'failed' : one.ended ? 'ended' : one.stopping ? 'stopping' : 'running';
    root.className = `card ${how}`;
    if (face.isConnected && !face.firstChild) face.replaceWith(pictured(one.program));
    name.textContent = one.program;
    told.replaceChildren(one.told.length ? h('span', {}, 'started with ', h('code', {}, one.told.join(' '))) : 'started with no arguments');
    const lasted = one.ended ? `after ${age(one.seconds)}` : `for ${age(one.seconds)}`;
    // What still ran when triq serve ended, ended with it.
    const does = view.gone && !one.ended ? 'ended with triq serve' : doing(one);
    status.textContent = `${does} · ${lasted} · process ${one.pid}`;
    // One that found its runtime's name taken is offered a name that is free.
    const unused = clash(one) && free(clash(one), state.runtimes);
    // The buttons are made again only when they change, or a click between two looks is lost.
    if (buttons !== how + unused) {
      buttons = how + unused;
      const stop = () => send('/api/stop', { id });
      const about = 'Its runtime did not shut down when it was asked: end the process';
      const renamed = () => start(one.program, named(one.told, unused));
      ends.replaceChildren(
        ...[
          how === 'running' && h('button.pill.stop', { onclick: stop }, 'Stop'),
          how === 'stopping' && h('button.pill.stop', { onclick: stop, title: about }, 'End it now'),
          unused && h('button.pill', { onclick: renamed, title: `The name ${clash(one)} is taken: start it under this one` }, `Start as ${unused}`),
          one.ended && !unused && h('button.pill.plain', { onclick: () => start(one.program, one.told.join(' ')), title: 'Start it as it was started' }, 'Start again'),
          one.ended && h('button.pill.plain', { onclick: () => send('/api/forget', { id }), title: 'Take it off the page' }, 'Clear'),
        ].filter(Boolean),
      );
    }
    // What one that failed printed is shown without asking, once: it says why.
    if (one.failed && !view.opened.has(id)) {
      view.opened.add(id);
      view.open.add(id);
    }
    const open = view.open.has(id);
    last.textContent = open ? '' : (one.printed[one.printed.length - 1] ?? 'it has printed nothing');
    output.textContent = open ? 'hide what it printed' : 'what it printed';
    // What is new is scrolled to, unless the person has scrolled back to read.
    const below = log.scrollHeight - log.scrollTop - log.clientHeight < 30;
    log.hidden = !open;
    log.textContent = one.printed.join('\n') || 'It has printed nothing.';
    if (open && below) log.scrollTop = log.scrollHeight;
  };
  return { root, update };
}

// Those that were started here, the newest first; and the runtimes that were started elsewhere.
function drawRunning() {
  const { started } = state.programs;
  for (const [id, card] of view.cards) {
    if (started.some((one) => one.id === id)) continue;
    card.root.remove();
    view.cards.delete(id);
  }
  for (const one of started) {
    if (!view.cards.has(one.id)) {
      view.cards.set(one.id, startedCard(one.id));
      $('running').prepend(view.cards.get(one.id).root);
    }
    view.cards.get(one.id).update(one);
  }
  const runs = started.filter((one) => !one.ended).length;
  $('nothing').hidden = started.length > 0;
  $('clear').hidden = !started.some((one) => one.ended);
  $('stop-all').hidden = runs === 0 || view.gone;
  $('quit').hidden = view.gone;
  $('headline').textContent = view.gone ? 'triq serve has ended' : runs === 0 ? 'Nothing is running' : `${runs} ${runs === 1 ? 'program is' : 'programs are'} running`;

  const elsewhere = others(state.runtimes);
  const drawn = JSON.stringify(elsewhere);
  if (drawn === view.elsewhere) return;
  view.elsewhere = drawn;
  const other = (each) =>
    h(
      'span.other',
      { title: `process ${each.pid}` },
      h('b', {}, each.runtime),
      `${each.nodes} ${each.nodes === 1 ? 'node' : 'nodes'}`,
      h('button', { onclick: () => confirm(`Shut down ${each.runtime}? It was not started here.`) && send('/api/shutdown', { runtime: each.runtime }), title: 'Ask this runtime to shut down' }, 'Shut down'),
    );
  $('others').replaceChildren(...(elsewhere.length ? ['Started elsewhere, and running:', ...elsewhere.map(other)] : []));
}

function draw() {
  drawRunning();
  drawPrograms();
  // What went wrong is said once, when it is new.
  const [count, problem] = state.said;
  if (count !== view.shown) {
    view.shown = count;
    if (problem) say(problem);
  }
}

async function refresh() {
  try {
    state = await api('/api/state');
    view.missed = 0;
    ended(false);
    draw();
  } catch (problem) {
    // Another triq serve answers here now, with a token of its own: this is its page to show.
    if (problem.status === 403) return location.reload();
    if (++view.missed >= 3) ended(true);
  }
}

$('search').addEventListener('input', () => {
  view.search = $('search').value;
  drawPrograms(true);
});
// Enter starts the program, when what was typed leaves only one.
$('search').addEventListener('keydown', (key) => {
  const left = families(state.programs.found, view.search).flatMap((each) => each.programs);
  if (key.key === 'Enter' && left.length === 1) start(left[0], another(left[0], state.programs.started, state.runtimes));
});
$('clear').addEventListener('click', () => send('/api/forget', {}));
$('stop-all').addEventListener('click', () => {
  const runs = state.programs.started.filter((one) => !one.ended);
  if (runs.length > 1 && !confirm(`Stop the ${runs.length} programs that were started here?`)) return;
  for (const one of runs) send('/api/stop', { id: one.id });
});
$('quit').addEventListener('click', async () => {
  if (!confirm('End triq serve, and the programs it started?')) return;
  await api('/api/quit', {}).catch((problem) => say(problem.message));
  ended(true);
});
mark($('mark'), 'TRIQ');
dissolve($('edge'), getComputedStyle(document.documentElement).getPropertyValue('--page').trim());
refresh();
setInterval(refresh, 500);
