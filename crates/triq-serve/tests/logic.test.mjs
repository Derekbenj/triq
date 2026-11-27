// What the page works out from what it is told, tried with node: `node --test`.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { age, another, batch, clash, commands, completed, doing, earlier, families, family, fieldsOf, found, free, hint, named, offers, others, registered, running, said, sheet, shown, standing } from '../assets/logic.js';

// A program that was started, as the page is told of it.
const one = (id, program, told, more = {}) => ({ id, program, told: told ? told.split(' ') : [], pid: 100 + id, runtime: null, nodes: null, seconds: 5, stopping: false, ended: null, failed: false, printed: [], ...more });
const home = (runtime, started = null) => ({ runtime, pid: 1, nodes: 0, started });

test('a program is of the family its name begins with, and the programs are listed family by family', () => {
  assert.deepEqual(['algorithm-stereo-depth', 'workbench', 'stack-quadcopter'].map(family), ['algorithm', '', 'stack']);
  const programs = ['algorithm-sensors', 'algorithm-stereo-depth', 'bench', 'stack-quadcopter', 'workbench'];
  assert.deepEqual(families(programs, ''), [
    { family: '', programs: ['bench', 'workbench'] },
    { family: 'algorithm', programs: ['algorithm-sensors', 'algorithm-stereo-depth'] },
    { family: 'stack', programs: ['stack-quadcopter'] },
  ]);
  // Searched, a family with nothing found in it is not listed.
  assert.deepEqual(families(programs, 'STEREO alg'), [{ family: 'algorithm', programs: ['algorithm-stereo-depth'] }]);
  assert.deepEqual(families(programs, 'nothing called this'), []);
  // One family can be chosen, also that of the programs that have none; the search is within it.
  assert.deepEqual(families(programs, '', 'algorithm'), [{ family: 'algorithm', programs: ['algorithm-sensors', 'algorithm-stereo-depth'] }]);
  assert.deepEqual(families(programs, '', ''), [{ family: '', programs: ['bench', 'workbench'] }]);
  assert.deepEqual(families(programs, 'work', ''), [{ family: '', programs: ['workbench'] }]);
  assert.deepEqual(families(programs, 'sensors', 'stack'), []);
  assert.ok(found('', 'anything') && found('quad  STACK', 'stack-quadcopter') && !found('quad x', 'stack-quadcopter'));
});

test('of a program, those that run are counted, and what it was told before is offered again', () => {
  const started = [one(1, 'bevy', 'bevy --fps 120'), one(2, 'bevy', '', { ended: 'stopped' }), one(3, 'xbox', ''), one(4, 'bevy', 'bevy --fps 120'), one(5, 'bevy', 'two --headless')];
  assert.deepEqual(running(started, 'bevy').map((each) => each.id), [1, 4, 5]);
  assert.deepEqual(running(started, 'workbench'), []);
  // The newest first, each once, and not the times it was told nothing.
  assert.deepEqual(earlier(started, 'bevy'), ['two --headless', 'bevy --fps 120']);
});

test('one more of a program that is a runtime is told a name that no runtime has', () => {
  // Nothing of it runs: it is told nothing, and takes the name it takes.
  assert.equal(another('stack-quadcopter', [], []), '');
  assert.equal(another('stack-quadcopter', [one(1, 'stack-quadcopter', '', { ended: 'ended' })], []), '');
  // One runs as a runtime, told nothing: the next is told the name with a number.
  const first = one(1, 'stack-quadcopter', '', { runtime: 'quadcopter' });
  assert.equal(another('stack-quadcopter', [first], [home('quadcopter', 1)]), 'quadcopter2');
  // A name that is taken, by whatever started it, is passed over; what else the last was told is kept.
  const second = one(2, 'stack-quadcopter', 'quadcopter2 --speed 5', { runtime: 'quadcopter2' });
  const machine = [home('quadcopter', 1), home('quadcopter2', 2), home('quadcopter3')];
  assert.equal(another('stack-quadcopter', [first, second], machine), 'quadcopter4 --speed 5');
  // What it was told that is no name stays where it is, behind the name.
  const fast = one(3, 'physics-rapier', '--speed 2', { runtime: 'physics' });
  assert.equal(another('physics-rapier', [fast], [home('physics', 3)]), 'physics2 --speed 2');
  // The name it began with is free again: that one.
  assert.equal(another('stack-quadcopter', [second], [home('quadcopter2', 2)]), 'quadcopter --speed 5');
  // One that is no runtime needs no name: the next is told what the last was.
  assert.equal(another('workbench', [one(4, 'workbench', '')], []), '');
  assert.equal(another('talker', [one(5, 'talker', 'loud --twice')], []), 'loud --twice');
});

test('one that ended because its name was taken is known by what triq said, and is offered a free one', () => {
  // As triq says it, and as a program that returns the error from its main prints it.
  const plain = one(1, 'algorithm-sensors', '', { ended: 'ended with status 1', failed: true, printed: ['a runtime named "sensors" is already running'] });
  const quoted = { ...plain, printed: ['starting', 'Error: Name("a runtime named \\"sensors\\" is already running (ExceedsMaxSupportedServers)")'] };
  assert.deepEqual([clash(plain), clash(quoted)], ['sensors', 'sensors']);
  // One that failed for another reason, and one that runs, found no name taken.
  assert.equal(clash({ ...plain, printed: ['Error: no controller'] }), null);
  assert.equal(clash({ ...plain, ended: null, failed: false }), null);
  // The name with the first number that leaves it free; and where it goes in what a program is told.
  assert.equal(free('sensors', [home('sensors'), home('sensors2')]), 'sensors3');
  assert.equal(free('sensors7', []), 'sensors');
  assert.deepEqual([named([], 'a'), named(['--fps', '60'], 'a'), named(['old', '--fps', '60'], 'a')], ['a', 'a --fps 60', 'a --fps 60']);
});

test('what one that was started is doing is said in words, and for how long', () => {
  assert.equal(doing(one(1, 'x', '')), 'running');
  assert.equal(doing(one(1, 'x', '', { runtime: 'quadcopter', nodes: 16 })), 'running as quadcopter, 16 nodes');
  assert.equal(doing(one(1, 'x', '', { runtime: 'bevy', nodes: 1 })), 'running as bevy, 1 node');
  assert.equal(doing(one(1, 'x', '', { runtime: 'bevy', nodes: 1, stopping: true })), 'stopping');
  assert.equal(doing(one(1, 'x', '', { ended: 'ended with status 3', failed: true })), 'ended with status 3');
  assert.deepEqual([0, 59, 60, 3599, 3600, 7500].map(age), ['0 s', '59 s', '1 min', '59 min', '1 h', '2 h 5 min']);
});

test('the runtimes that were started elsewhere are those no program started here is', () => {
  assert.deepEqual(others([home('quadcopter', 1), home('bevy'), home('xbox')]).map((each) => each.runtime), ['bevy', 'xbox']);
});

// What the tool says it can be told, as far as the tests need it; and a machine of two runtimes.
const USAGE = `usage:
  triq list                                   runtimes, factories, nodes and connections
  triq spawn <runtime> <kind> <name> [--pool <pool>] [--params <json>]
  triq connect <runtime/node/output> <runtime/node/input>
  triq pause <runtime/node>
  triq shutdown <runtime>
  triq <program> [what it is told]            run triq-<program>, as triq workbench runs triq-workbench

programs here, each run as triq <its name>:
  workbench`;
const carried = (name) => ({ name, size: 8, align: 8, slice: false, header: '' });
const output = (runtime, node, port, message) => ({ port: { name: port, message: carried(message) }, service: `${runtime}/${node}/${port}`, subscribers: 0, sent: 0 });
const input = (port, message) => ({ port: { name: port, message: carried(message) }, sources: [] });
const MACHINE = [
  { runtime: 'desk', pid: 2, pools: [], factories: [], nodes: [{ name: 'viewer', kind: '', state: 'Paused', inputs: [input('pose', 'Pose'), input('count', 'u64')], outputs: [] }] },
  {
    runtime: 'robot',
    pid: 1,
    pools: [{ name: 'dsp', threads: 2 }],
    factories: [{ kind: 'filter' }, { kind: 'fuser' }],
    nodes: [
      { name: 'imu', kind: '', state: 'Running', inputs: [], outputs: [output('robot', 'imu', 'pose', 'Pose'), output('robot', 'imu', 'ticks', 'u64')] },
      { name: 'smoother', kind: 'filter', state: { Stopped: 'it panicked' }, inputs: [input('raw', 'Pose')], outputs: [output('robot', 'smoother', 'smooth', 'Pose')] },
    ],
  },
];

test('what the tool can be told is read from what it says, and said back for the command being typed', () => {
  const known = commands(USAGE);
  assert.deepEqual(known.map((each) => each.command), ['list', 'spawn', 'connect', 'pause', 'shutdown']);
  assert.deepEqual(known[0], { command: 'list', takes: '', about: 'runtimes, factories, nodes and connections' });
  assert.equal(known[1].takes, '<runtime> <kind> <name> [--pool <pool>] [--params <json>]');
  assert.equal(hint('triq connect robot/imu', known), 'triq connect <runtime/node/output> <runtime/node/input>');
  assert.equal(hint('list', known), 'triq list: runtimes, factories, nodes and connections');
  // While its name is still being typed, the one command it can become; with several, or none, nothing.
  assert.equal(hint('sp', known), 'triq spawn <runtime> <kind> <name> [--pool <pool>] [--params <json>]');
  assert.deepEqual([hint('s', known), hint('workbench', known), hint('', known)], [null, null, null]);
});

test('what is being typed is offered its ending: a command, a program, or a name from the machine', () => {
  const known = commands(USAGE).map((each) => each.command);
  const offer = (line) => offers(line, known, ['workbench', 'stack-quadcopter'], MACHINE);
  assert.deepEqual(offer('s'), ['spawn', 'shutdown', 'stack-quadcopter']);
  assert.deepEqual(offer('triq pa'), ['pause']);
  assert.deepEqual(offer('shutdown '), ['desk', 'robot']);
  assert.deepEqual(offer('pause robot/'), ['robot/imu', 'robot/smoother']);
  // A kind of the runtime that was named, and then its pools after the word that takes one.
  assert.deepEqual(offer('spawn robot f'), ['filter', 'fuser']);
  assert.deepEqual(offer('spawn robot filter f2 --pool '), ['dsp']);
  assert.deepEqual(offer('spawn robot filter f2 -'), ['--pool', '--params']);
  // An output to wire from, and then only the inputs that take what it sends.
  assert.deepEqual(offer('connect robot/imu'), ['robot/imu/pose', 'robot/imu/ticks']);
  assert.deepEqual(offer('connect robot/imu/pose '), ['desk/viewer/pose', 'robot/smoother/raw']);
  assert.deepEqual(offer('connect robot/imu/ticks '), ['desk/viewer/count']);
  // Nothing is offered that is typed already, or after a program's name.
  assert.deepEqual([offer('shutdown robot'), offer('workbench '), offer('list ')], [[], [], []]);
  // An offer taken stands in the place of what was being typed, with room for the next word.
  assert.equal(completed('triq connect robot/imu', 'robot/imu/pose'), 'triq connect robot/imu/pose ');
  assert.equal(completed('sp', 'spawn'), 'spawn ');
});

test('what the tool printed is read as what it is', () => {
  const printed = ['message  F64   at byte 0    translation[0]', 'message  F64   at byte 8    translation[1]', 'header   U32   at byte 0    width'];
  assert.deepEqual(fieldsOf(printed), [
    { part: 'message', kind: 'F64', at: 0, name: 'translation[0]' },
    { part: 'message', kind: 'F64', at: 8, name: 'translation[1]' },
    { part: 'header', kind: 'U32', at: 0, name: 'width' },
  ]);
  assert.equal(fieldsOf(['triq: "robot/imu" is not `runtime/node/output`']), null);
  // What went wrong is the tool's first words, without its name; what it can be told, printed after, is left out.
  assert.deepEqual(said(['triq: no command `lst`, and no program triq-lst beside triq', 'usage:', '  triq list']), ['no command `lst`, and no program triq-lst beside triq']);
  assert.deepEqual(said(['registered /robot/target: its programs are run by name from now on']), ['registered /robot/target: its programs are run by name from now on']);
  // How a node stands, in a word.
  assert.deepEqual(MACHINE.flatMap((home) => home.nodes).map(standing), ['paused', 'running', 'stopped']);
});

test('the cheat sheet has the commands the tool has, by what they are for, each with a line to try', () => {
  const known = [...commands(USAGE), { command: 'frobnicate', takes: '<thing>', about: 'a command of a newer tool' }];
  const parts = sheet(known, MACHINE, ['workbench', 'stack-quadcopter']);
  assert.deepEqual(parts.map((part) => part.title), ['Look', 'Make and remove', 'Wire', 'Change', 'Stop', 'Programs', 'More']);
  const entry = (command) => parts.flatMap((part) => part.entries).find((each) => each.command === command);
  // What a command takes is what the tool says; what it is for is said in a sentence.
  assert.equal(entry('spawn').takes, '<runtime> <kind> <name> [--pool <pool>] [--params <json>]');
  assert.ok(entry('connect').about.length > 20);
  // The lines to try are made of the names on the machine, so that they can be run as they are.
  assert.equal(entry('list').example, 'list');
  assert.equal(entry('spawn').example, 'spawn robot filter filter1');
  assert.equal(entry('connect').example, 'connect robot/imu/pose desk/viewer/pose');
  assert.equal(entry('pause').example, 'pause desk/viewer');
  assert.equal(entry('shutdown').example, 'shutdown desk');
  // A program is started by its name; and a command this page has no words for is listed with the tool's own.
  assert.deepEqual(entry('<program>'), { command: '<program>', takes: '[arguments]', about: entry('<program>').about, example: 'workbench' });
  assert.deepEqual(entry('frobnicate'), { command: 'frobnicate', takes: '<thing>', about: 'a command of a newer tool', example: 'frobnicate' });
  // A command the tool does not have is not on the sheet.
  assert.equal(entry('group'), undefined);
  // With nothing running, the lines to try are of names that say what goes where.
  const bare = sheet(known, [], []);
  const of = (command) => bare.flatMap((part) => part.entries).find((each) => each.command === command).example;
  assert.deepEqual([of('connect'), of('pause'), of('spawn')], ['connect robot/imu/pose desk/viewer/pose', 'pause robot/filter', "spawn robot filter filter1 --params '{\"alpha\": 0.5}'"]);
  assert.ok(!bare.some((part) => part.title === 'Programs' && part.entries.length === 0));
});

test('the page is shown dark or light as was chosen, or as the system is', () => {
  // Nothing chosen, or something this page does not know, is dark.
  assert.deepEqual([shown(null, true), shown('', true), shown('sepia', true)], ['dark', 'dark', 'dark']);
  assert.deepEqual([shown('dark', true), shown('light', false)], ['dark', 'light']);
  // Left to the system, it is as the system is, and changes with it.
  assert.deepEqual([shown('auto', true), shown('auto', false)], ['light', 'dark']);
});

test('what is pasted is the lines to run, one after another', () => {
  const pasted = `# a world with one body in it
physics-rapier
  triq spawn physics multirotor quad --params '{"position": [0, 0, -1]}'

$ triq pause physics/quad\r
   # and nothing more
`;
  assert.deepEqual(batch(pasted), ['physics-rapier', `triq spawn physics multirotor quad --params '{"position": [0, 0, -1]}'`, 'triq pause physics/quad']);
  // A single line is a single line, with whatever ends it; and nothing is nothing.
  assert.deepEqual([batch('list\n'), batch('  \n\n')], [['list'], []]);
});

test('the folders that are registered are read from what the tool says', () => {
  const some = `${USAGE}

looked for beside triq, in the folders registered, and on the PATH
folders registered in /Users/someone/.config/triq/folders:
  /Users/someone/robot/target/release
  /opt/my robots/bin
`;
  assert.deepEqual(registered(some), { file: '/Users/someone/.config/triq/folders', folders: ['/Users/someone/robot/target/release', '/opt/my robots/bin'] });
  const none = `${USAGE}\n\nlooked for beside triq, in the folders registered, and on the PATH\nno folder is registered in /home/someone/.config/triq/folders\n`;
  assert.deepEqual(registered(none), { file: '/home/someone/.config/triq/folders', folders: [] });
  // A tool that says nothing of folders has none to show.
  assert.deepEqual(registered(USAGE), { file: '', folders: [] });
});
