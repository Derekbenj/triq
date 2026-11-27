// The page: it asks twice a second which programs there are and which of them run, draws that,
// and sends what is clicked and what is typed. It starts programs and stops them, and has a
// line for the commands of the triq tool. To draw what is inside a runtime is the workbench's.
import { mark } from '/hero.js';
import { age, another, batch, clash, commands, completed, doing, earlier, families, family, fieldsOf, free, hint, line, named, offers, options, others, registered, running, said, sheet, shown, shows, standing } from '/logic.js';

const token = document.querySelector('meta[name=triq-token]').content;
const $ = (id) => document.getElementById(id);

// What the server last said, and what the person has done on the page.
let state = { programs: { found: [], started: [], asked: [] }, runtimes: [], said: [0, null] };
const view = {
  // What is typed to find a program, and the one family whose programs are shown, if one is
  // chosen: the empty name is the family of those that have none.
  search: '',
  of: undefined,
  families: '',
  // What each program says it takes, in its own words: asked for when its box for arguments
  // is first opened. Null while it is being asked; `said` is null if it does not say.
  takes: new Map(),
  // The same for what was started: what is typed to find some, and the family chosen.
  running: { search: '', of: undefined, families: undefined },
  // What a program is to be told, as typed in its box, and the programs whose box is open.
  told: {},
  boxes: new Set(),
  // The cards of those that were started, by their numbers; whose output is open; and those
  // whose output was opened for them once, because they failed.
  cards: new Map(),
  open: new Set(),
  opened: new Set(),
  // The one whose output is in a window in front of the page, by its number, if one is.
  expanded: null,
  // What the list of programs, the runtimes of elsewhere and the counts were last drawn from,
  // so that they are drawn again only when that changes; and how many problems were said.
  drawn: '',
  elsewhere: '',
  counted: '',
  shown: 0,
  // How many times in a row the server did not answer; whether it is taken to have ended;
  // and whether the page asked it to, stopping what it had started: 'stopped' then.
  missed: 0,
  gone: false,
  quit: null,
  // The command line. What the tool can be told; the machine as it was last looked at, and
  // when; the lines that were typed, the newest last, each with what came of it, and how each
  // was last drawn; the nodes of a list whose ports are shown; and what to bring back with
  // the arrow keys.
  known: [],
  machine: [],
  looked: 0,
  lines: [],
  answers: new Map(),
  unfolded: new Set(),
  before: [],
  recall: 0,
  // The line that is being done: the next waits for it, so that lines are done in their order.
  doing: Promise.resolve(),
  // Lines that were pasted together, waiting to be run one after another.
  batch: [],
  // Which of the two is under the line: the answers, or the cheat sheet.
  tab: 'answers',
  // Which of the two the page shows: the programs, or the command line.
  page: 'programs',
  // How the page is to be shown: dark, light, or auto for as the system is set.
  theme: 'dark',
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
  const stopped = 'triq serve has ended, and the programs it had started were stopped. Run triq serve again, and this page comes back.';
  const left = 'triq serve has ended. What it had started does not depend on it, and runs on: run triq serve again, and this page comes back with it.';
  if (gone) say(view.quit === 'stopped' ? stopped : left, true);
  else [$('said').hidden, view.quit] = [true, null];
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

// An element: `h('button.button', { onclick }, 'Start')`.
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

// Each family of programs has a colour: its name is written in it, and its programs have an edge of it.
const COLOURS = { '': '#c7afff', algorithm: '#38d6ff', peripheral: '#ffd23f', physics: '#62f08a', simulator: '#ff9a3d', stack: '#ff5fa8' };
const SPARE = ['#7de3d0', '#c7afff', '#ff9a3d', '#38d6ff', '#ff5fa8'];
const colourOf = (name) => {
  const letters = [...family(name)].reduce((sum, letter) => sum + letter.codePointAt(0), 0);
  return COLOURS[family(name)] ?? SPARE[letters % SPARE.length];
};
// A program's name without its family, which is said above it.
const short = (name) => (family(name) ? name.slice(family(name).length + 1) : name);

// What a program says it takes, asked of the server the first time it is wanted.
async function takes(name) {
  if (!view.takes.has(name)) {
    view.takes.set(name, null);
    const answer = await api('/api/takes', { program: name }).catch(() => ({ takes: null }));
    view.takes.set(name, { said: answer.takes });
  }
  return view.takes.get(name);
}

// Under a box for arguments: what the program says it takes, and each option it names as
// something to click, which `put` then puts in the box.
function takesOf(name, put) {
  const known = view.takes.get(name);
  if (!known) return h('div.takes', {}, h('p.note', {}, 'It is being asked what it takes.'));
  if (!known.said) return h('div.takes', {}, h('p.note', {}, 'This program does not say what it takes. It would if it answered --help.'));
  const option = (each) => h('button.told', { type: 'button', onclick: () => put(each), title: 'Put this with the arguments' }, each);
  const named = options(known.said);
  return h('div.takes', {}, h('pre', {}, known.said), named.length > 0 && h('div.before', {}, named.map(option)));
}
// What is in a box for arguments, with one more word at its end.
const added = (typed, more) => `${typed.trimEnd()} ${more} `.trimStart();

// Draw the programs again though one of their boxes is being typed in, and leave the keys
// where they were.
function redrawPrograms() {
  const at = document.activeElement?.dataset?.told;
  drawPrograms(true);
  const box = at && document.querySelector(`[data-told="${at}"]`);
  if (!box) return;
  box.focus();
  box.setSelectionRange(box.value.length, box.value.length);
}

// A program that can be started. With others of it running, one more is started with what the
// last was, under a name that is free: that is said on the card before it is done.
function programCard(name) {
  const { started } = state.programs;
  const runs = running(started, name).length;
  const proposed = another(name, started, state.runtimes);
  const asked = view.boxes.has(name);
  const go = () => start(name, asked ? (view.told[name] ?? '') : proposed);
  const ask = () => {
    if (asked) view.boxes.delete(name);
    else view.boxes.add(name);
    if (!asked && !view.told[name]) view.told[name] = proposed;
    drawPrograms(true);
    document.querySelector(`[data-told="${name}"]`)?.focus();
    // What it takes is under the box as soon as it has said.
    if (!asked) takes(name).then(redrawPrograms);
  };
  const more = (option) => {
    view.told[name] = added(view.told[name] ?? '', option);
    drawPrograms(true);
    const box = document.querySelector(`[data-told="${name}"]`);
    box?.focus();
    box?.setSelectionRange(box.value.length, box.value.length);
  };
  const box = h('input', { type: 'text', 'data-told': name, value: view.told[name] ?? '', placeholder: 'as after its name in a terminal: a name, --speed 5', spellcheck: 'false', autocomplete: 'off' });
  box.addEventListener('input', () => (view.told[name] = box.value));
  box.addEventListener('keydown', (key) => key.key === 'Enter' && go());
  const again = (told) => h('button.told', { title: 'It was started with these before', onclick: () => ((view.told[name] = told), drawPrograms(true)) }, told);
  // One line for a program: its name, how many of it run, and what starts it. More only when
  // there is more to say: what the next would be started with, and the box for arguments.
  return h(
    'div.card.item',
    {},
    h(
      'div.row',
      {},
      h('div.name.grow', { title: `triq ${name}` }, short(name)),
      runs > 0 && h('span.badge', {}, `${runs} running`),
      h('button.link', { onclick: ask, title: 'What would be typed after its name in a terminal' }, asked ? 'no arguments' : 'arguments'),
      h('button.button.go.small', { onclick: go, title: `triq ${name}` }, runs ? 'Start another' : 'Start'),
    ),
    !asked && proposed && h('div.under', {}, 'the next is started with ', h('span.told', {}, proposed)),
    asked && h('div.tell', {}, box),
    asked && earlier(started, name).length > 0 && h('div.before', {}, earlier(started, name).map(again)),
    asked && takesOf(name, more),
  );
}

// The programs, family by family. They are drawn again when something they show has changed,
// and not while something is being typed in one of their boxes.
function drawPrograms(now = false) {
  const { found, started } = state.programs;
  // The families there are, to choose one of. They are listed again when they change.
  const all = families(found, '').map((each) => each.family);
  if (all.join() !== view.families) {
    view.families = all.join();
    if (!all.includes(view.of)) view.of = undefined;
    const option = (of) => h('option', { value: of, selected: of === view.of }, of || 'triq');
    $('of').replaceChildren(h('option', { value: '*' }, 'every family'), ...all.map(option));
  }
  const listed = families(found, view.search, view.of);
  const shows = found.map((name) => [running(started, name).length, another(name, started, state.runtimes), earlier(started, name)]);
  const drawn = JSON.stringify([listed, shows, [...view.boxes]]);
  if (!now && (drawn === view.drawn || $('programs').contains(document.activeElement))) return;
  view.drawn = drawn;
  const none = found.length
    ? 'No program has that in its name.'
    : 'No program of triq was found. They are looked for beside the triq tool, in the folders registered with triq register <folder>, and on the PATH.';
  const parts = listed.map(({ family: of, programs }) =>
    h('div.family', { style: `--colour: ${colourOf(programs[0])}` }, h('h3', {}, of || 'triq'), h('div.cards', {}, programs.map(programCard))),
  );
  $('programs').replaceChildren(...(parts.length ? parts : [h('p.none', {}, none)]));
}

// How one that was started stands, in a line: what it is doing, for how long, and its process.
function stateOf(one) {
  const lasted = one.ended ? `after ${age(one.seconds)}` : `for ${age(one.seconds)}`;
  // What still ran when triq serve ended was stopped with it, if that was asked: or runs on.
  const without = view.quit === 'stopped' ? 'stopped with triq serve' : 'left running';
  const does = view.gone && !one.ended ? without : doing(one);
  return `${does} · ${lasted} · process ${one.pid}`;
}

// What one that was started printed, in a window in front of the page that is as large as the
// page allows, and collapsed to its card again. It has more of it than the card has, and is
// kept up to date for as long as it is there.
function expand(id) {
  view.expanded = id;
  $('printed-text').textContent = '';
  $('printed').showModal();
  drawPrinted();
}

async function drawPrinted() {
  if (view.expanded === null) return;
  const one = state.programs.started.find((one) => one.id === view.expanded);
  // One that was taken off the page has nothing more to show.
  if (!one) return $('printed').close();
  $('printed').className = one.failed ? 'failed' : one.ended ? 'ended' : one.stopping ? 'stopping' : 'running';
  $('printed-name').textContent = one.program;
  $('printed-state').textContent = `${stateOf(one)} · started with ${line(one.told) || 'no arguments'}`;
  $('printed-where').textContent = `Its last lines. All of it is in ${one.log}`;
  // Without the server there is what the card has.
  const { printed } = await api('/api/printed', { id: one.id }).catch(() => one);
  const [text, chosen] = [$('printed-text'), document.getSelection()];
  // It is left as it is while some of it is selected, so that it can be copied.
  const held = !chosen.isCollapsed && text.contains(chosen.anchorNode);
  const now = printed.join('\n') || 'It has printed nothing.';
  if (view.expanded !== one.id || held || text.textContent === now) return;
  // What is new is scrolled to, unless the person has scrolled back to read.
  const below = text.scrollHeight - text.scrollTop - text.clientHeight < 30;
  text.textContent = now;
  if (below) text.scrollTop = text.scrollHeight;
}

// The card of one that was started. It is made once and kept, so that its output stays where
// it was scrolled to; `update` brings it in line with what the server says of it.
function startedCard(id) {
  const [status, last, log] = [h('div.state'), h('span'), h('pre.log', { hidden: true })];
  const [name, told, ends] = [h('div.name'), h('div.sub'), h('div.row')];
  // For one that has ended: a box to start it again with other arguments than it had, since
  // some programs only say what they take when they are started with nothing.
  const other = h('input', { type: 'text', spellcheck: 'false', autocomplete: 'off', placeholder: 'as after its name in a terminal' });
  const again = h('form.tell', { hidden: true }, other, h('button.button.go', {}, 'Start'));
  const hints = h('div', { hidden: true });
  const output = h('button.link', {
    onclick: () => {
      if (view.open.has(id)) view.open.delete(id);
      else view.open.add(id);
      draw();
    },
  });
  const larger = h('button.link', { onclick: () => expand(id), title: 'What it printed, in a window as large as the page: more of it than is here' }, 'expand');
  const root = h('div.card', {}, h('div.row', {}, h('div.grow', {}, name, told), ends), again, hints, status, h('div.last', {}, last, larger, output), log);
  let buttons = '';
  const update = (one) => {
    const how = one.failed ? 'failed' : one.ended ? 'ended' : one.stopping ? 'stopping' : 'running';
    root.className = `card ${how}`;
    root.style.setProperty('--colour', colourOf(one.program));
    // One that a triq serve before this one started, and left running, is here by its record.
    const before = one.adopted && h('span.badge.kept', { title: 'A triq serve before this one started it, and left it running' }, 'from before');
    name.replaceChildren(one.program, before || '');
    told.replaceChildren(one.told.length ? h('span', {}, 'started with ', h('span.told', {}, one.told.join(' '))) : 'started with no arguments');
    status.textContent = stateOf(one);
    // One that found its runtime's name taken is offered a name that is free.
    const unused = clash(one) && free(clash(one), state.runtimes);
    // The buttons are made again only when they change, or a click between two looks is lost.
    if (buttons !== how + unused) {
      buttons = how + unused;
      const stop = () => send('/api/stop', { id });
      const about = 'Its runtime did not shut down when it was asked: end the process';
      const renamed = () => start(one.program, named(one.told, unused));
      // The box opens with what it was started with, to be added to or changed.
      const more = (option) => {
        other.value = added(other.value, option);
        other.focus();
      };
      const argue = () => {
        [again.hidden, hints.hidden] = [!again.hidden, !again.hidden];
        if (again.hidden) return;
        other.value ||= line(one.told);
        other.focus();
        // Under the box, what the program says it takes.
        hints.replaceChildren(takesOf(one.program, more));
        takes(one.program).then(() => hints.replaceChildren(takesOf(one.program, more)));
      };
      again.onsubmit = (sent) => {
        sent.preventDefault();
        [again.hidden, hints.hidden] = [true, true];
        start(one.program, other.value);
      };
      if (!one.ended) [again.hidden, hints.hidden] = [true, true];
      ends.replaceChildren(
        ...[
          how === 'running' && h('button.button.stop', { onclick: stop }, 'Stop'),
          how === 'stopping' && h('button.button.stop', { onclick: stop, title: about }, 'End it now'),
          unused && h('button.button.go', { onclick: renamed, title: `The name ${clash(one)} is taken: start it under this one` }, `Start as ${unused}`),
          one.ended && !unused && h('button.button', { onclick: () => start(one.program, line(one.told)), title: 'Start it as it was started' }, 'Start again'),
          one.ended && h('button.button', { onclick: argue, title: 'Start it again with other arguments than it had' }, 'With arguments'),
          one.ended && h('button.button', { onclick: () => send('/api/forget', { id }), title: 'Take it off the page' }, 'Clear'),
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
    log.title = `All of it is in ${one.log}`;
    if (open && below) log.scrollTop = log.scrollHeight;
  };
  return { root, update };
}

// Those that were started here, the newest first; the runtimes that were started elsewhere;
// and what is said of both at the top.
function drawRunning() {
  const { started, found } = state.programs;
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
    // Only those are shown that are like what was typed, and of the family that was chosen.
    view.cards.get(one.id).root.hidden = !shows(one, view.running.search, view.running.of);
  }
  // The families of what was started, to choose one of. They are listed again when they change.
  const all = families(started.map((one) => one.program), '').map((each) => each.family);
  // Those of no family are a family too, the one with no name: so the list is told apart as JSON.
  if (JSON.stringify(all) !== view.running.families) {
    view.running.families = JSON.stringify(all);
    if (!all.includes(view.running.of)) view.running.of = undefined;
    const option = (of) => h('option', { value: of, selected: of === view.running.of }, of || 'triq');
    $('running-of').replaceChildren(h('option', { value: '*' }, 'every family'), ...all.map(option));
  }
  $('running-finding').hidden = started.length === 0;
  $('none-shown').hidden = started.length === 0 || started.some((one) => shows(one, view.running.search, view.running.of));
  const runs = started.filter((one) => !one.ended).length;
  const elsewhere = others(state.runtimes);
  $('nothing').hidden = started.length > 0;
  $('clear').hidden = !started.some((one) => one.ended);
  $('stop-all').hidden = runs === 0 || view.gone;
  $('quit').hidden = view.gone;
  $('headline').textContent = view.gone ? 'triq serve has ended' : runs === 0 ? 'Nothing is running' : `${runs} ${runs === 1 ? 'program is' : 'programs are'} running`;
  const counted = [runs, elsewhere.length, found.length].map((count) => String(count).padStart(2, '0'));
  if (counted.join() !== view.counted) {
    view.counted = counted.join();
    const count = (number, of) => h('span', {}, h('b', {}, number), of);
    $('counts').replaceChildren(count(counted[0], 'running, started here'), count(counted[1], 'started elsewhere'), count(counted[2], 'programs to start'));
  }

  const drawn = JSON.stringify(elsewhere);
  if (drawn === view.elsewhere) return;
  view.elsewhere = drawn;
  const shut = (each) => confirm(`Shut down ${each.runtime}? It was not started here.`) && send('/api/shutdown', { runtime: each.runtime });
  const other = (each) =>
    h(
      'span.other',
      { title: `process ${each.pid}` },
      h('b', {}, each.runtime),
      `${each.nodes} ${each.nodes === 1 ? 'node' : 'nodes'}`,
      h('button', { onclick: () => shut(each), title: 'Ask this runtime to shut down' }, 'Shut down'),
    );
  $('others').replaceChildren(...(elsewhere.length ? ['Started elsewhere, and running:', ...elsewhere.map(other)] : []));
}

// ---- The command line ----

// Look at the whole machine: for a list, and for what is offered as a line is typed.
async function look() {
  view.looked = Date.now();
  view.machine = await api('/api/machine').catch(() => view.machine);
  return view.machine;
}

// What a port carries, as the tool writes it.
const carried = ({ message }) => `${message.name}${message.slice ? '[]' : ''}${message.header ? ` + ${message.header}` : ''}`;

// A node as a row of a list. A click shows its ports under it: what each input hears, and how
// many inputs each output is heard by.
function nodeRow(entry, home, node) {
  const key = `${entry.key}|${home.runtime}/${node.name}`;
  const fold = () => {
    if (view.unfolded.has(key)) view.unfolded.delete(key);
    else view.unfolded.add(key);
    drawAnswers();
  };
  const where = node.placement === null ? 'run by its program' : node.placement === 'Dedicated' ? 'own thread' : `pool ${node.placement.Pool}`;
  // A node that its program runs itself is not counted by the runtime.
  const runs = node.placement === null ? '' : `${node.runs} runs`;
  const row = h(`div.node.${standing(node)}`, { onclick: fold, title: `${standing(node)}, ${where}: click for its ports` }, h('span', {}, node.name), h('span.dim', {}, node.kind || where), h('span.dim', {}, runs));
  if (!view.unfolded.has(key)) return row;
  const heard = (source) => h('div', {}, `← ${source.output}: ${source.received} received, ${source.lost} lost, ${source.latency_us} us`);
  const inputs = node.inputs.map((input) => h('div', {}, h('b', {}, `in ${input.port.name}`), ` ${carried(input.port)}`, input.sources.map(heard)));
  const outputs = node.outputs.map((output) => h('div', {}, h('b', {}, `out ${output.port.name}`), ` ${carried(output.port)} → ${output.subscribers} ${output.subscribers === 1 ? 'input' : 'inputs'}, ${output.sent} sent`));
  const panic = typeof node.state !== 'string' && h('div', {}, `stopped by a panic: ${node.state.Stopped}`);
  return [row, h('div.ports', {}, panic, inputs, outputs, inputs.length + outputs.length === 0 && 'it has no ports')];
}

// The machine as `triq list` tells it, drawn: each runtime, the kinds it offers, and its nodes.
function listed(entry) {
  if (entry.machine.length === 0) return [h('p.dim', {}, 'No runtime is running on this machine.')];
  return entry.machine.map((home) => {
    const pools = home.pools.map((pool) => `pool ${pool.name} ×${pool.threads}`).join(', ');
    const kinds = home.factories.map((factory) => factory.kind).join(', ');
    return h(
      'div.home',
      {},
      h('div.home-name', {}, h('b', {}, home.runtime), `pid ${home.pid}`, pools, h('span', {}, `${home.nodes.length} ${home.nodes.length === 1 ? 'node' : 'nodes'}`)),
      kinds && h('div.ports', {}, h('b', {}, 'kinds '), kinds),
      home.nodes.map((node) => nodeRow(entry, home, node)),
    );
  });
}

// A table: its headings, and its rows. A long one is scrolled within itself.
const table = (headings, rows) => {
  const head = h('tr', {}, headings.map((heading) => h('th', {}, heading)));
  return h('div.long', {}, h('table', {}, head, rows.map((row) => h('tr', {}, row.map((cell) => h('td', {}, cell))))));
};

// The command the tool was run with for a line, as the server last told of it: it forgets a
// command after a while, and the line then keeps what was last seen of it.
const seen = (entry) => (entry.seen = state.programs.asked.find((one) => one.id === entry.id) ?? entry.seen);

// What came of a line that was typed: how it is drawn, and something that changes when it is
// to be drawn again.
function answered(entry) {
  if (entry.kind === 'list') {
    const unfolded = [...view.unfolded].filter((key) => key.startsWith(`${entry.key}|`));
    return { note: entry.live ? 'kept up to date' : '', shape: `${entry.stamp}|${unfolded}`, parts: () => listed(entry) };
  }
  if (entry.kind === 'json') return { note: '', shape: 'json', parts: () => [h('pre', {}, JSON.stringify(entry.machine, null, 2))] };
  // Of lines that were pasted together, the first that fails is the last that is run.
  if (entry.rest) {
    const rest = h('div.rest', {}, `${entry.rest === 1 ? 'the line' : `the ${entry.rest} lines`} after it ${entry.rest === 1 ? 'was' : 'were'} not run`);
    const { note, shape, parts } = answered({ ...entry, rest: 0 });
    return { note, shape: `${shape}|rest`, parts: () => [...parts(), rest] };
  }
  if (entry.kind === 'wrong') return { note: '', shape: 'wrong', parts: () => [h('div.wrong', {}, entry.text)] };
  if (entry.kind === 'sent') return { note: '', shape: 'sent', parts: () => [h('div.wait', {}, 'sent')] };
  if (entry.kind === 'started') {
    const one = state.programs.started.find((one) => one.id === entry.id);
    return { note: '', shape: `started ${one?.program}`, parts: () => [h('div.done', {}, `started${one ? ` ${one.program}` : ''} as a program: it is under Running`)] };
  }
  // A command the tool was run with. It is forgotten by the server after a while, and is then
  // shown as it was last seen.
  const one = seen(entry);
  if (!one) return { note: '', shape: 'gone', parts: () => [] };
  const printed = said(one.printed);
  const parts = () => {
    if (!one.ended) return [h('div.wait', {}, 'running'), printed.length > 0 && h('pre', {}, printed.join('\n'))];
    if (one.failed) return [h('div.wrong', {}, h('pre', {}, printed.join('\n') || one.ended))];
    const fields = one.told[0] === 'fields' && fieldsOf(one.printed);
    if (fields) return [table(['in', 'stored as', 'at byte', 'number'], fields.map((field) => [field.part, field.kind, String(field.at), field.name]))];
    const usage = one.told[0] === 'help' && commands(one.printed.join('\n'));
    if (usage) return [table(['command', 'takes', 'for'], usage.map((each) => [each.command, each.takes, each.about]))];
    return [printed.length ? h('pre', {}, printed.join('\n')) : h('div.done', {}, 'done')];
  };
  return { note: one.ended ? '' : 'running', shape: `${one.ended}|${one.printed.length}`, parts };
}

// The lines that were typed, the newest first, each with what came of it. One is drawn again
// when what came of it has changed.
function drawAnswers() {
  const box = $('answers');
  box.querySelector('.none').hidden = view.lines.length > 0;
  for (const [key, made] of view.answers) {
    if (view.lines.some((entry) => entry.key === key)) continue;
    made.root.remove();
    view.answers.delete(key);
  }
  for (const entry of view.lines) {
    if (!view.answers.has(entry.key)) {
      const [note, came] = [h('em'), h('div.came')];
      const root = h('div.answer', {}, h('div.typed', {}, h('span', {}, entry.line), note), came);
      view.answers.set(entry.key, { root, note, came, shape: null });
      box.prepend(root);
    }
    const [made, { note, shape, parts }] = [view.answers.get(entry.key), answered(entry)];
    made.note.textContent = note;
    if (shape === made.shape) continue;
    made.shape = shape;
    made.came.replaceChildren(...parts().flat(Infinity).filter(Boolean));
  }
}

// Wait until what a line asked for is done, so that the next line finds it done: until the
// tool has ended; or, for a program that was started, until it is a runtime that can be
// asked things, or has ended, or three seconds have passed, since not every program is one.
async function settled(entry) {
  const began = Date.now();
  for (;;) {
    await refresh();
    const program = entry.kind === 'started' && state.programs.started.find((one) => one.id === entry.id);
    const waits = entry.kind === 'asked' ? seen(entry) && !seen(entry).ended : program && !program.ended && !program.runtime && Date.now() - began < 3000;
    if (!waits || Date.now() - began > 60000) return;
    await new Promise((later) => setTimeout(later, 120));
  }
}

// Whether what a line asked for went wrong.
const failed = (entry) => {
  if (entry.kind === 'wrong') return true;
  if (entry.kind === 'asked') return Boolean(seen(entry)?.failed);
  return entry.kind === 'started' && Boolean(state.programs.started.find((one) => one.id === entry.id)?.failed);
};

// Run lines one after another, each when the one before it is done. The first that fails is
// the last: what comes after it was written for a machine on which it had worked.
async function runAll(lines) {
  for (const [at, line] of lines.entries()) {
    const entry = await run(line);
    if (entry && failed(entry) && at + 1 < lines.length) {
      entry.rest = lines.length - at - 1;
      drawAnswers();
      return;
    }
  }
}

// The lines that were pasted together, shown before they are run.
function drawBatch() {
  $('batch').hidden = view.batch.length === 0;
  const go = () => {
    const lines = view.batch;
    view.batch = [];
    drawBatch();
    show('answers');
    view.doing = view.doing.then(() => runAll(lines));
  };
  const forget = () => {
    view.batch = [];
    drawBatch();
  };
  const about = `${view.batch.length} lines were pasted. They are run one after another, each when the one before it is done, and the first that fails is the last.`;
  $('batch').replaceChildren(about, h('pre', {}, view.batch.join('\n')), h('div.row', {}, h('button.button.go', { type: 'button', onclick: go }, 'Run them'), h('button.button', { type: 'button', onclick: forget }, 'Forget them')));
}

// Do what a line says, and wait until it is done. The list is drawn here from what the
// runtimes report, which is what the tool would print; a program's name starts the program;
// the rest is for the tool.
async function run(line) {
  const typed = line.trim().replace(/^triq(\s+|$)/, '');
  if (!typed) return;
  view.before.push(typed);
  view.recall = view.before.length;
  if (typed === 'clear') return wipe();
  // It is on the page at once, in the order it was typed in, and what comes of it follows.
  const entry = { key: view.before.length, line: typed, kind: 'sent', stamp: 0 };
  // A list that was kept up to date is left as it is once something else is typed.
  for (const other of view.lines) other.live = false;
  view.lines = [...view.lines, entry].slice(-40);
  $('answers').scrollTop = 0;
  drawAnswers();
  if (['list', 'watch', 'json'].includes(typed)) {
    Object.assign(entry, { machine: await look(), kind: typed === 'json' ? 'json' : 'list', live: typed === 'watch' });
  } else {
    try {
      const { started, asked } = await api('/api/command', { line: typed });
      Object.assign(entry, started ? { kind: 'started', id: started } : { kind: 'asked', id: asked });
    } catch (problem) {
      Object.assign(entry, { kind: 'wrong', text: problem.message });
    }
  }
  await settled(entry);
  return entry;
}

function wipe() {
  view.lines = [];
  view.unfolded.clear();
  drawAnswers();
}

// The cheat sheet: what the tool can be told, with lines to try that are made of the names on
// the machine. A click on one puts it on the line, to be changed or run as it is.
function drawSheet() {
  const put = (line) => {
    $('line').value = line;
    $('line').focus();
    suggest();
  };
  const cheat = (each) =>
    h(
      'div.cheat',
      {},
      h('div.takes', {}, h('b', {}, each.command), ` ${each.takes}`),
      h('p', {}, each.about),
      h('button.try', { type: 'button', onclick: () => put(each.example), title: 'Put this on the line' }, each.example),
    );
  const parts = sheet(view.known, view.machine, state.programs.found).map((part) => [h('h3', {}, part.title), part.entries.map(cheat)]);
  const keys = h('p.keys', {}, 'On the line: Tab takes what is offered, the arrows bring back what was typed before, quotes are as in a terminal, and ', h('code', {}, 'clear'), ' empties the answers.');
  $('sheet').replaceChildren(...parts.flat(Infinity), keys);
}

// Show the answers or the cheat sheet under the line. The sheet is made as it is shown, of
// the machine as it is then.
function show(tab) {
  view.tab = tab;
  for (const name of ['answers', 'sheet']) {
    $(name).hidden = name !== tab;
    $(`tab-${name}`).setAttribute('aria-selected', String(name === tab));
  }
  $('wipe').hidden = tab !== 'answers';
  if (tab === 'sheet') look().then(drawSheet);
}

// Show the programs, or the command line, which then has the whole width of the page. Which
// it was is remembered in this browser, for the next time the page is opened.
function turn(page) {
  view.page = page === 'line' ? 'line' : 'programs';
  for (const tab of $('pages').children) tab.setAttribute('aria-selected', String(tab.value === view.page));
  $('page-programs').hidden = view.page !== 'programs';
  $('page-line').hidden = view.page !== 'line';
  if (view.page === 'line') $('line').focus();
  try {
    localStorage.setItem('triq-page', view.page);
  } catch (refused) {
    // A browser that keeps nothing shows the programs the next time.
  }
}

// The page is dark, or light, or as the system is set, whichever was chosen: that is
// remembered in this browser. Left to the system, it changes when the system does.
const system = window.matchMedia('(prefers-color-scheme: light)');
function theme(chosen) {
  view.theme = ['light', 'auto'].includes(chosen) ? chosen : 'dark';
  document.documentElement.dataset.theme = shown(view.theme, system.matches);
  for (const choice of $('theme').children) choice.setAttribute('aria-checked', String(choice.value === view.theme));
  try {
    localStorage.setItem('triq-theme', view.theme);
  } catch (refused) {
    // A browser that keeps nothing shows the page dark the next time.
  }
}
const remembered = (what) => {
  try {
    return localStorage.getItem(what);
  } catch (refused) {
    return null;
  }
};

// Under the line as it is typed: what the command takes, and the names that could end the
// word being typed. Tab takes as much as they all begin with, or the first.
function suggest() {
  const line = $('line').value;
  $('hint').textContent = hint(line, view.known) ?? '';
  const known = view.known.map((each) => each.command);
  const names = line.trim() ? offers(line, known, state.programs.found, view.machine).slice(0, 10) : [];
  $('offers').replaceChildren(...names.map((name) => h('button', { type: 'button', onclick: () => take(name) }, name)));
  return names;
}
function take(name, whole = true) {
  const taken = completed($('line').value, name);
  $('line').value = whole ? taken : taken.trimEnd();
  $('line').focus();
  suggest();
}

function draw() {
  drawRunning();
  drawPrograms();
  drawAnswers();
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
    // The list that is kept up to date is looked at again once a second.
    const live = view.lines.find((entry) => entry.live);
    if (live && Date.now() - view.looked > 900) Object.assign(live, { machine: await look(), stamp: live.stamp + 1 });
    ended(false);
    draw();
    drawPrinted();
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
$('running-find').addEventListener('input', () => {
  view.running.search = $('running-find').value;
  drawRunning();
});
$('running-of').addEventListener('change', () => {
  view.running.of = $('running-of').value === '*' ? undefined : $('running-of').value;
  drawRunning();
});
// Enter starts the program, when what was typed leaves only one.
$('of').addEventListener('change', () => {
  view.of = $('of').value === '*' ? undefined : $('of').value;
  drawPrograms(true);
});
$('search').addEventListener('keydown', (key) => {
  const left = families(state.programs.found, view.search, view.of).flatMap((each) => each.programs);
  if (key.key === 'Enter' && left.length === 1) start(left[0], another(left[0], state.programs.started, state.runtimes));
});
$('clear').addEventListener('click', () => send('/api/forget', {}));
$('stop-all').addEventListener('click', () => {
  const runs = state.programs.started.filter((one) => !one.ended);
  if (runs.length > 1 && !confirm(`Stop the ${runs.length} programs that were started here?`)) return;
  for (const one of runs) send('/api/stop', { id: one.id });
});
// Quitting. What was started does not depend on triq serve: it is stopped with it, or left
// running for the next triq serve to find, as is chosen.
$('quit').addEventListener('click', () => {
  const runs = state.programs.started.filter((one) => !one.ended).length;
  const some = `${runs === 1 ? 'One program that was started here is' : `${runs} programs that were started here are`} running. ${runs === 1 ? 'It does' : 'They do'} not depend on triq serve: stop ${runs === 1 ? 'it' : 'them'} with it, or leave ${runs === 1 ? 'it' : 'them'} running, and the next triq serve finds ${runs === 1 ? 'it' : 'them'} again.`;
  $('quitting-about').textContent = runs ? some : 'Nothing that was started here is running.';
  $('quit-stop').hidden = runs === 0;
  $('quit-stop').textContent = runs === 1 ? 'Stop it and quit' : 'Stop them and quit';
  $('quit-leave').textContent = runs === 0 ? 'Quit' : runs === 1 ? 'Leave it running' : 'Leave them running';
  $('quitting').showModal();
});
async function quit(leave) {
  $('quitting').close();
  await api('/api/quit', { leave }).catch((problem) => say(problem.message));
  view.quit = leave ? 'left' : 'stopped';
  ended(true);
}
$('quit-stop').addEventListener('click', () => quit(false));
$('quit-leave').addEventListener('click', () => quit(true));
$('quit-not').addEventListener('click', () => $('quitting').close());

// ---- The settings ----

// Run the tool with a line, off the answers, and wait for it: what it said, once it has ended.
async function tool(line) {
  const { asked } = await api('/api/command', { line });
  for (;;) {
    await refresh();
    const one = state.programs.asked.find((one) => one.id === asked);
    if (!one || one.ended) return one;
    await new Promise((later) => setTimeout(later, 120));
  }
}
// A folder's name as one word of a line: what would end the word has a backslash before it.
// It is not put in quotes, so that a ~ it begins with is still the person's own folder.
const word = (folder) => folder.replace(/([\s'"\\])/g, '\\$1');

// The folders that are registered, as the tool tells them, each with what takes it out again;
// and the folder the records of what was started are in.
async function drawSettings() {
  const { usage } = await api('/api/usage');
  view.known = commands(usage ?? '');
  const { file, folders } = registered(usage ?? '');
  const folder = (each) => h('div.row.folder', {}, h('span.told.grow', { title: each }, each), h('button.button.small', { onclick: () => register(`unregister ${word(each)}`) }, 'Remove'));
  $('folders').replaceChildren(...(folders.length ? folders.map(folder) : [h('p.note', {}, 'No folder is registered.')]));
  $('registry').textContent = file ? `They are the lines of ${file}, which triq register writes too.` : '';
  if (document.activeElement !== $('records')) $('records').value = (await api('/api/records')).folder;
}
// Register a folder or take one out, by the tool, and say what it said if that went wrong.
async function register(line) {
  $('settings-said').textContent = '';
  const one = await tool(line).catch((problem) => ({ failed: true, printed: [problem.message] }));
  if (one?.failed) $('settings-said').textContent = said(one.printed).join(' ');
  await drawSettings();
  return !one?.failed;
}
$('settings-open').addEventListener('click', () => {
  [$('settings-said').textContent, $('kept-said').textContent] = ['', ''];
  $('settings').showModal();
  drawSettings();
});
$('settings-close').addEventListener('click', () => $('settings').close());
// What a program printed is collapsed to its card again by its button, by Esc, and by a click
// beside the window.
$('printed-close').addEventListener('click', () => $('printed').close());
$('printed').addEventListener('close', () => (view.expanded = null));
$('printed').addEventListener('click', (click) => {
  const within = $('printed').getBoundingClientRect();
  const beside = click.clientX < within.left || click.clientX > within.right || click.clientY < within.top || click.clientY > within.bottom;
  if (click.target === $('printed') && beside) $('printed').close();
});
$('register').addEventListener('submit', async (sent) => {
  sent.preventDefault();
  const folder = $('folder').value.trim();
  if (folder && (await register(`register ${word(folder)}`))) $('folder').value = '';
});
$('keep').addEventListener('submit', async (sent) => {
  sent.preventDefault();
  [$('settings-said').textContent, $('kept-said').textContent] = ['', ''];
  try {
    $('records').value = (await api('/api/records', { folder: $('records').value })).folder;
    $('kept-said').textContent = 'What is started is kept there from now on, and what was kept before was moved there.';
  } catch (problem) {
    $('settings-said').textContent = problem.message;
  }
});

for (const choice of $('theme').children) choice.addEventListener('click', () => theme(choice.value));
system.addEventListener('change', () => theme(view.theme));
$('tab-answers').addEventListener('click', () => show('answers'));
$('tab-sheet').addEventListener('click', () => show('sheet'));
$('wipe').addEventListener('click', wipe);
$('typing').addEventListener('submit', (sent) => {
  sent.preventDefault();
  const line = $('line').value;
  // What comes of the line is among the answers.
  if (line.trim()) show('answers');
  view.doing = view.doing.then(() => run(line));
  $('line').value = '';
  suggest();
});
$('line').addEventListener('input', suggest);
// Several lines pasted at once are not one line: they are shown, to be run one after another.
$('line').addEventListener('paste', (pasted) => {
  const lines = batch(pasted.clipboardData?.getData('text') ?? '');
  if (lines.length < 2) return;
  pasted.preventDefault();
  view.batch = lines;
  drawBatch();
});
// The names that are offered are those of the machine as it is when the line is typed on.
$('line').addEventListener('focus', () => look().then(suggest));
$('line').addEventListener('keydown', (key) => {
  if (key.key === 'Tab') {
    const names = suggest();
    if (names.length === 0) return;
    key.preventDefault();
    // As much as all of them begin with, if that is more than is typed; or else the first.
    const shared = names.reduce((a, b) => a.slice(0, [...a].findIndex((letter, at) => letter !== b[at]) >>> 0));
    const piece = $('line').value.split(/\s+/).pop();
    if (names.length > 1 && shared.length > piece.length) take(shared, false);
    else take(names[0]);
  } else if (key.key === 'ArrowUp' || key.key === 'ArrowDown') {
    // What was typed before comes back, and what was typed after it.
    key.preventDefault();
    view.recall = Math.min(view.before.length, Math.max(0, view.recall + (key.key === 'ArrowUp' ? -1 : 1)));
    $('line').value = view.before[view.recall] ?? '';
    suggest();
  }
});

mark($('mark'), 'TRIQ');
theme(remembered('triq-theme'));
for (const tab of $('pages').children) tab.addEventListener('click', () => turn(tab.value));
turn(remembered('triq-page'));
api('/api/usage')
  .then(({ usage }) => (view.known = commands(usage ?? '')))
  .catch(() => {});
refresh();
setInterval(refresh, 500);
