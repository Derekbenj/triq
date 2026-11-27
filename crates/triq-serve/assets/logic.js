// What the page works out from what it is told. Nothing here touches the page, so that it can
// be tried without one: see tests/logic.test.mjs.

// Whether a name has every word that was typed in it.
export const found = (search, name) => {
  const words = search.toLowerCase().split(/\s+/).filter(Boolean);
  return words.every((word) => name.toLowerCase().includes(word));
};

// A program is of the family its name begins with: algorithm-sensors is of the family
// algorithm. One whose name is a single word is of none.
export const family = (name) => (name.includes('-') ? name.slice(0, name.indexOf('-')) : '');

// The programs that have what was typed in their names, family by family, those of no family
// first. With a family given, only that one: the empty name is the family of those with none.
export function families(programs, search, of) {
  const listed = [];
  for (const name of programs.filter((name) => found(search, name) && (of === undefined || family(name) === of))) {
    const known = listed.find((each) => each.family === family(name));
    if (known) known.programs.push(name);
    else listed.push({ family: family(name), programs: [name] });
  }
  return listed.sort((a, b) => a.family.localeCompare(b.family));
}

// Those of a program that were started and still run.
export const running = (started, program) => started.filter((each) => each.program === program && !each.ended);

// A word as it is typed for a terminal to take it whole: as it is, if nothing in it would
// end it; in quotes, if something would; and if it has such a quote itself, with a backslash
// before each letter that would.
const quoted = (word) => {
  if (/^[^\s'"\\]+$/.test(word)) return word;
  return word.includes("'") ? word.replace(/([\s'"\\])/g, '\\$1') : `'${word}'`;
};

// What a program was told, as a line that tells it the same again.
export const line = (told) => told.map(quoted).join(' ');

// What a program was told before, the newest first and each once: to be told it again.
export function earlier(started, program) {
  const told = started.filter((each) => each.program === program && each.told.length > 0);
  return [...new Set(told.map((each) => line(each.told)).reverse())];
}

// What one more of a program is told, while others of it run: what the last of them was told.
// If they are runtimes it is given a name as well, since two runtimes cannot have one name:
// the name of the first, with the first number that leaves it free. The name goes where a
// program of triq takes it, as the first word it is told.
export function another(program, started, runtimes) {
  const runs = running(started, program);
  if (runs.length === 0) return '';
  const told = [...runs[runs.length - 1].told];
  const first = runs.find((each) => each.runtime);
  return first ? named(told, free(first.runtime, runtimes)) : line(told);
}

// A name that no runtime has: the one given without the number it may end in, or that with the
// first number, from 2, that leaves it free.
export function free(name, runtimes) {
  const taken = new Set(runtimes.map((each) => each.runtime));
  const stem = name.replace(/\d+$/, '') || name;
  let unused = stem;
  for (let number = 2; taken.has(unused); number++) unused = stem + number;
  return unused;
}

// What a program is told, with a name for its runtime: in the place of the first word, if that
// is no option, since that is the name it was given before; or else in front.
export function named(told, name) {
  const rest = told.length > 0 && !told[0].startsWith('-') ? told.slice(1) : told;
  return [name, line(rest)].join(' ').trim();
}

// The name one that was started found taken, if that is what it ended of: triq says so in
// these words, which a program that returns the error from its main prints inside quotes.
export function clash(one) {
  const said = one.failed && one.printed.join('\n').match(/a runtime named \\?"([^"\\]+)\\?" is already running/);
  return said ? said[1] : null;
}

// What one that was started is doing, in words.
export function doing(one) {
  if (one.ended) return one.ended;
  if (one.stopping) return 'stopping';
  if (!one.runtime) return 'running';
  return `running as ${one.runtime}, ${one.nodes} ${one.nodes === 1 ? 'node' : 'nodes'}`;
}

// For how long, in the largest units that say it.
export function age(seconds) {
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)} h${minutes % 60 ? ` ${minutes % 60} min` : ''}`;
}

// The runtimes that no program started here is: they were started elsewhere.
export const others = (runtimes) => runtimes.filter((each) => each.started === null);

// What the tool can be told, read from what it says when asked: of each command its name, what
// it takes, and what it is for. The line about programs is no command of its own.
export function commands(usage) {
  const lines = usage.split('\n').filter((line) => /^ {2}triq [a-z]/.test(line));
  return lines.map((line) => {
    const [, command, takes = '', about = ''] = line.match(/^ {2}triq (\S+) ?(.*?)(?: {2,}(.*))?$/);
    return { command, takes: takes.trim(), about: about.trim() };
  });
}

// The words of a line as far as it is typed, without the tool's name: the last is the one
// being typed, and is empty after a space.
const typedWords = (line) => line.replace(/^\s*(triq(\s+|$))?/, '').split(/\s+/);

// What the tool takes for the command that is being typed, to be shown under the line. While
// the command's own name is being typed, that is the one command it can still become.
export function hint(line, known) {
  const [first, ...rest] = typedWords(line);
  const becomes = known.filter((each) => (rest.length ? each.command === first : first && each.command.startsWith(first)));
  if (becomes.length !== 1) return null;
  const [{ command, takes, about }] = becomes;
  return takes ? `triq ${command} ${takes}` : `triq ${command}: ${about}`;
}

// What a port carries, to tell whether an output and an input go together.
const carries = (port) => JSON.stringify(port.message);

// The names that can stand where the last word of a line is being typed, in the order they are
// offered in: a command or a program, or what the command takes in that place, from the machine.
export function offers(line, known, programs, machine) {
  const typed = typedWords(line);
  const [command, place, piece] = [typed[0], typed.length - 1, typed[typed.length - 1]];
  const home = machine.find((each) => each.runtime === typed[1]);
  const runtimes = machine.map((each) => each.runtime);
  const nodes = machine.flatMap((each) => each.nodes.map((node) => `${each.runtime}/${node.name}`));
  const outputs = machine.flatMap((each) => each.nodes.flatMap((node) => node.outputs));
  const inputs = machine.flatMap((each) => each.nodes.flatMap((node) => node.inputs.map((input) => ({ ...input, name: `${each.runtime}/${node.name}/${input.port.name}` }))));
  const from = outputs.find((output) => output.service === typed[1]);
  let names = [];
  if (place === 0) names = [...known, ...programs];
  else if (command === 'spawn' && place === 1) names = runtimes;
  else if (command === 'spawn' && place === 2) names = (home?.factories ?? []).map((factory) => factory.kind);
  else if (command === 'spawn' && typed[place - 1] === '--pool') names = (home?.pools ?? []).map((pool) => pool.name);
  else if (command === 'spawn' && place > 3 && piece.startsWith('-')) names = ['--pool', '--params'];
  else if (['connect', 'disconnect', 'fields'].includes(command) && place === 1) names = outputs.map((output) => output.service);
  // An input is offered to an output that sends what it takes.
  else if (['connect', 'disconnect'].includes(command) && place === 2) names = inputs.filter((input) => !from || carries(input.port) === carries(from.port)).map((input) => input.name);
  else if (['set', 'change', 'group', 'pause', 'resume', 'remove'].includes(command) && place === 1) names = nodes;
  else if (command === 'shutdown' && place === 1) names = runtimes;
  return names.filter((name) => name.startsWith(piece) && name !== piece);
}

// A line with an offer taken: it stands where the last word was being typed, with a space
// after it for the next.
export const completed = (line, offer) => `${line.slice(0, line.length - typedWords(line).pop().length)}${offer} `;

// What `triq fields` printed, as rows: which part of what is sent a number is in, how it is
// stored, where it begins, and its name. Null if what was printed is something else.
export function fieldsOf(printed) {
  const rows = printed.map((line) => line.match(/^(message|header)\s+(\w+)\s+at byte (\d+)\s+(.+)$/));
  if (printed.length === 0 || rows.some((row) => !row)) return null;
  return rows.map(([, part, kind, at, name]) => ({ part, kind, at: Number(at), name }));
}

// What the tool printed, as it is worth reading: without its own name in front of what went
// wrong, and without all it can be told, which it prints after a command it does not know.
export function said(printed) {
  const usage = printed.indexOf('usage:');
  return (usage < 0 ? printed : printed.slice(0, usage)).map((line) => line.replace(/^triq: /, ''));
}

// How a node stands, in a word: a runtime says `Running`, `Paused`, or `Stopped` with why.
export const standing = (node) => (typeof node.state === 'string' ? node.state.toLowerCase() : 'stopped');

// The cheat sheet: what each command of the tool is for, and which go together.
const SHEET = [
  ['Look', { list: 'Every runtime, the kinds of node it offers, and its nodes. Click a node for its ports.', watch: 'The list, kept up to date until something else is typed.', json: 'The same as JSON, as a program would read it.', fields: 'The numbers inside what an output sends, each by its name.' }],
  ['Make and remove', { spawn: 'Make a node of a kind that the runtime offers. Its settings are JSON, in quotes.', remove: 'Take a node away.' }],
  ['Wire', { connect: 'Have an input hear an output. Both must carry the same message.', disconnect: 'Cut that wire.' }],
  ['Change', { change: 'Change the settings named, and leave the others.', set: 'Give a node all of its settings anew: one that is left out is its default again.', group: 'Put a node in a group, or with no name in none.', pause: 'Keep a node from running.', resume: 'Let it run again.' }],
  ['Stop', { shutdown: 'Ask a runtime to shut down, and its program with it.' }],
  ['Programs', { register: 'Have the programs of a folder found from now on, with nothing on the PATH.', unregister: 'Take a folder out of those again.' }],
];

// A line to try for a command, made of names from the machine where it has some that fit, so
// that it can be run as it is; and otherwise of names that say what goes where.
function example(command, machine) {
  const nodes = machine.flatMap((home) => home.nodes.map((node) => ({ ...node, name: `${home.runtime}/${node.name}` })));
  const outputs = nodes.flatMap((node) => node.outputs);
  const inputs = nodes.flatMap((node) => node.inputs.map((input) => ({ ...input, name: `${node.name}/${input.port.name}` })));
  const node = nodes[0]?.name ?? 'robot/filter';
  const offers = machine.find((home) => home.factories.length > 0);
  // A wire that could be made: an output, and an input that takes what it sends. And one that is there.
  const pair = outputs.flatMap((output) => inputs.filter((input) => carries(input.port) === carries(output.port)).map((input) => `${output.service} ${input.name}`))[0];
  const wired = inputs.flatMap((input) => input.sources.map((source) => `${source.output} ${input.name}`))[0];
  const [setting, value] = Object.entries(nodes[0]?.params ?? {})[0] ?? ['alpha', 0.1];
  const lines = {
    fields: `fields ${outputs[0]?.service ?? 'robot/imu/pose'}`,
    spawn: offers ? `spawn ${offers.runtime} ${offers.factories[0].kind} ${offers.factories[0].kind}1` : `spawn robot filter filter1 --params '{"alpha": 0.5}'`,
    remove: `remove ${node}`,
    connect: `connect ${pair ?? 'robot/imu/pose desk/viewer/pose'}`,
    disconnect: `disconnect ${wired ?? pair ?? 'robot/imu/pose desk/viewer/pose'}`,
    change: `change ${node} '${JSON.stringify({ [setting]: value })}'`,
    set: `set ${node} '${JSON.stringify({ [setting]: value })}'`,
    group: `group ${node} sensing`,
    pause: `pause ${node}`,
    resume: `resume ${node}`,
    shutdown: `shutdown ${machine[0]?.runtime ?? 'robot'}`,
    register: 'register ~/robot/target/release',
    unregister: 'unregister ~/robot/target/release',
  };
  return lines[command] ?? command;
}

// The cheat sheet as it is shown: the commands the tool has, in parts by what they are for,
// each with what it takes, what it is for, and a line to try. A program is started by its
// name; and a command of a newer tool, which this has no words for, is listed last with the
// tool's own.
export function sheet(known, machine, programs) {
  const entry = (each, about) => ({ command: each.command, takes: each.takes, about, example: example(each.command, machine) });
  const parts = SHEET.map(([title, about]) => ({ title, entries: known.filter((each) => each.command in about).map((each) => entry(each, about[each.command])) }));
  if (programs.length > 0) {
    const about = 'A program is started by its name, as its button starts it, with what is typed after the name.';
    parts.find((part) => part.title === 'Programs').entries.unshift({ command: '<program>', takes: '[arguments]', about, example: programs[0] });
  }
  const said = new Set(SHEET.flatMap(([, about]) => Object.keys(about)));
  parts.push({ title: 'More', entries: known.filter((each) => !said.has(each.command)).map((each) => entry(each, each.about)) });
  return parts.filter((part) => part.entries.length > 0);
}

// How the page is shown, for what was chosen and whether the system is set light: dark or
// light as chosen, as the system is when that was left to it, and dark when nothing was chosen.
export const shown = (chosen, systemLight) => (chosen === 'light' || (chosen === 'auto' && systemLight) ? 'light' : 'dark');

// What was pasted, as the lines to run one after another: without the empty ones, without
// those that are a remark, which begin with #, and without the $ that a line copied from a
// terminal may begin with.
export const batch = (pasted) => {
  const lines = pasted.split(/\r?\n/).map((line) => line.trim().replace(/^\$\s+/, ''));
  return lines.filter((line) => line && !line.startsWith('#'));
};

// The folders that are registered for programs to be looked for in, and the file that lists
// them, read from what the tool says when asked what it can be told: it ends with them.
export function registered(usage) {
  const lines = usage.split('\n');
  const at = lines.findIndex((line) => /^(folders registered|no folder is registered) in /.test(line));
  if (at < 0) return { file: '', folders: [] };
  const file = lines[at].replace(/^(folders registered|no folder is registered) in /, '').replace(/:$/, '');
  const listed = lines.slice(at + 1).filter((line) => line.startsWith('  '));
  return { file, folders: listed.map((line) => line.trim()) };
}
