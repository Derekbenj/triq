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

// The programs that have what was typed in their names, family by family, those of no family first.
export function families(programs, search) {
  const listed = [];
  for (const name of programs.filter((name) => found(search, name))) {
    const known = listed.find((each) => each.family === family(name));
    if (known) known.programs.push(name);
    else listed.push({ family: family(name), programs: [name] });
  }
  return listed.sort((a, b) => a.family.localeCompare(b.family));
}

// Those of a program that were started and still run.
export const running = (started, program) => started.filter((each) => each.program === program && !each.ended);

// What a program was told before, the newest first and each once: to be told it again.
export function earlier(started, program) {
  const told = started.filter((each) => each.program === program && each.told.length > 0);
  return [...new Set(told.map((each) => each.told.join(' ')).reverse())];
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
  return first ? named(told, free(first.runtime, runtimes)) : told.join(' ');
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
  return [name, ...rest].join(' ');
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

// A picture for a name: five rows of five squares, each there or not, the same on both sides.
// The same name always has the same picture, so that a program is known by it at a glance.
export function picture(name) {
  // A number made of the name, mixed until each of its bits depends on every letter.
  let number = 2166136261;
  for (const letter of name) number = Math.imul(number ^ letter.codePointAt(0), 16777619);
  number = Math.imul(number ^ (number >>> 15), 2246822519);
  number = (number ^ (number >>> 13)) >>> 0;
  const rows = [0, 1, 2, 3, 4].map((down) => {
    const [a, b, c] = [0, 1, 2].map((across) => ((number >>> (down * 3 + across)) & 1) === 1);
    return [a, b, c, b, a];
  });
  // A picture with next to nothing in it is turned inside out.
  const sparse = rows.flat().filter(Boolean).length < 9;
  return sparse ? rows.map((row) => row.map((on) => !on)) : rows;
}
