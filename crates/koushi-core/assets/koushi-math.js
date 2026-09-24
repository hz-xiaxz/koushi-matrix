"use strict";
// Renders `data-mx-maths` sources with the bundled KaTeX when a Koushi history
// export page is opened. The limits match the timeline's
// (apps/desktop/src/components/timeline/TimelineMessageBody.tsx).
document.addEventListener("DOMContentLoaded", function () {
  if (typeof katex === "undefined") {
    return;
  }
  var maxSourceLength = 1024;
  var nodes = document.querySelectorAll("[data-mx-maths]");
  for (var index = 0; index < nodes.length; index += 1) {
    var node = nodes[index];
    var source = (node.getAttribute("data-mx-maths") || "").trim();
    if (!source || source.length > maxSourceLength) {
      continue;
    }
    try {
      katex.render(source, node, {
        displayMode: node.tagName === "DIV",
        strict: false,
        throwOnError: false,
        trust: false,
        maxExpand: 1000,
        maxSize: 20
      });
    } catch (error) {
      // Keep the sender's fallback text.
    }
  }
});
