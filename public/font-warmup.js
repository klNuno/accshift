// Started by index.html with the hint src/lib/app/fontWarmup.ts left at the
// end of the last start. Measuring that text in those fonts makes the renderer
// open the same font files the page's first layout is about to need, on this
// thread instead of the page's.
self.onmessage = (event) => {
  try {
    const { fonts, text } = JSON.parse(event.data);
    const context = new OffscreenCanvas(1, 1).getContext("2d");
    for (const font of fonts.slice(0, 16)) {
      context.font = String(font);
      context.measureText(String(text).slice(0, 256));
    }
  } catch {
    // A stale or unreadable hint only means the fonts load at layout, as
    // they would without it.
  }
  self.close();
};
