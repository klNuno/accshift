/** Reveals an avatar `<img>` once its bytes are decoded.
 *
 * The element starts at `opacity: 0` (component CSS) so a picture that is still
 * downloading shows the neutral avatar box instead of a half-painted frame, then
 * fades in. The reveal is an inline style rather than a class: Svelte prunes
 * scoped selectors it cannot see in the markup, and a class added from an action
 * is invisible to that analysis.
 */
export function fadeInOnLoad(node: HTMLImageElement) {
  const reveal = () => {
    node.style.opacity = "1";
  };

  // Cached image: `load` already fired before the action ran.
  if (node.complete && node.naturalWidth > 0) {
    reveal();
    return;
  }

  node.addEventListener("load", reveal);
  // A broken URL must never leave a permanently invisible avatar.
  node.addEventListener("error", reveal);

  return {
    destroy() {
      node.removeEventListener("load", reveal);
      node.removeEventListener("error", reveal);
    },
  };
}
