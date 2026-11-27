// The top of the page: the name in squares, with colours running along it, and the edge under
// it, where the colour breaks up into squares. Flat squares, drawn by the page itself.

// Each letter on five squares by seven.
const LETTERS = {
  T: ['#####', '..#..', '..#..', '..#..', '..#..', '..#..', '..#..'],
  R: ['####.', '#...#', '#...#', '####.', '#.#..', '#..#.', '#...#'],
  I: ['#####', '..#..', '..#..', '..#..', '..#..', '..#..', '#####'],
  Q: ['.###.', '#...#', '#...#', '#...#', '#.#.#', '#..#.', '.##.#'],
};

// Write a word in squares into an element that lays its children out on a grid. Each square
// is behind the one on its left by a moment, so that what the style sheet has every square
// do, change colour and hop, runs along the word as a wave.
export function mark(element, word) {
  let column = 1;
  for (const letter of word) {
    LETTERS[letter].forEach((row, down) => {
      [...row].forEach((dot, across) => {
        if (dot !== '#') return;
        const square = document.createElement('span');
        square.style.gridColumn = column + across;
        square.style.gridRow = down + 1;
        square.style.animationDelay = `${-(column + across) * 0.11 - down * 0.06}s`;
        element.append(square);
      });
    });
    column += 6;
  }
}

// Draw the edge on a canvas that has one pixel for each square, and is shown large. More of
// the squares are of the page's colour lower down and further right, so the edge climbs to the
// right in steps; a square near the edge comes and goes, slowly.
export function dissolve(canvas, colour, square = 12) {
  const context = canvas.getContext('2d');
  const still = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  // A number between 0 and 1 for a place, always the same for the same place.
  const chance = (across, down) => {
    let number = Math.imul(across * 374761393 + down * 668265263, 1274126177);
    number ^= number >>> 15;
    return ((number >>> 0) % 1000) / 1000;
  };
  const draw = (time) => {
    const [columns, rows] = [Math.ceil(canvas.clientWidth / square), Math.ceil(canvas.clientHeight / square)];
    if (canvas.width !== columns || canvas.height !== rows) [canvas.width, canvas.height] = [columns, rows];
    context.clearRect(0, 0, columns, rows);
    context.fillStyle = colour;
    for (let down = 0; down < rows; down++) {
      for (let across = 0; across < columns; across++) {
        const level = (down / (rows - 1)) * 1.7 + (across / columns) * 1.1 - 0.95;
        const sway = still ? 0 : 0.07 * Math.sin(time / 1300 + chance(down + 7, across + 3) * 6.283);
        // Smoothly there over a tenth of the way, so that a square fades and does not blink.
        const there = Math.min(1, Math.max(0, (level + sway - chance(across, down)) / 0.1));
        if (there > 0) {
          context.globalAlpha = there;
          context.fillRect(across, down, 1, 1);
        }
      }
    }
    if (!still) requestAnimationFrame(draw);
  };
  requestAnimationFrame(draw);
  if (still) window.addEventListener('resize', () => requestAnimationFrame(draw));
}
