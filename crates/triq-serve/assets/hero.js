// The name at the top of the page, as on an arcade machine: big letters of squares, each over
// the edge of the next and a little out of line, and a jiggle when the pointer moves over it.
// How the letters look is the style sheet's; here they are made and moved.

// Write a word into an element, a letter to a span, and have the letters jiggle under the pointer.
export function mark(element, word) {
  element.replaceChildren(...[...word].map((letter) => Object.assign(document.createElement('span'), { textContent: letter })));
  if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  // Each letter is put a little off where it stands, anew as the pointer moves, and let go a
  // moment after the pointer stops: the style sheet has it spring back.
  const off = (most) => ((Math.random() - 0.5) * 2 * most).toFixed(1);
  let [last, rest] = [0, undefined];
  element.addEventListener('pointermove', (moved) => {
    if (moved.timeStamp - last < 45) return;
    last = moved.timeStamp;
    for (const letter of element.children) {
      letter.style.setProperty('--dx', `${off(4)}px`);
      letter.style.setProperty('--dy', `${off(5)}px`);
      letter.style.setProperty('--turn', `${off(7)}deg`);
    }
    clearTimeout(rest);
    rest = setTimeout(() => {
      for (const letter of element.children) ['--dx', '--dy', '--turn'].forEach((moved) => letter.style.removeProperty(moved));
    }, 110);
  });
}
