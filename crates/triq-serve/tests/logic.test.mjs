// What the page works out from what it is told, tried with node: `node --test`.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { age, another, clash, doing, earlier, families, family, found, free, named, others, picture, running } from '../assets/logic.js';

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

test('a program has a picture of its own: five rows of five squares, the same on both sides, and always the same', () => {
  for (const name of ['workbench', 'stack-quadcopter', 'algorithm-sensors', 'x']) {
    const squares = picture(name);
    assert.equal(squares.length, 5);
    assert.ok(squares.every((row) => row.length === 5 && row[0] === row[4] && row[1] === row[3]));
    assert.ok(squares.flat().filter(Boolean).length >= 6, `${name} is not nearly empty`);
    assert.deepEqual(picture(name), squares);
  }
  assert.notDeepEqual(picture('algorithm-sensors'), picture('algorithm-estimators'));
});
