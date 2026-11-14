// Render SVG or HTML animation frames to PNG with Playwright Chromium.
// Usage: node render_frames.mjs plan.json
// The plan carries the input, viewport, animation preset and frame times.
// Animations are paused and seeked per frame, so output is deterministic.
const fs = require("fs");
const { pathToFileURL } = require("url");

function loadPlaywright() {
  try {
    return require("playwright");
  } catch (err) {
    throw new Error("playwright not found; run: npm install -g playwright && npx playwright install chromium");
  }
}

function presetCss(animate, duration) {
  const table = {
    rotate: {
      frames: "@keyframes gif-rotate{from{transform:rotate(0deg)}to{transform:rotate(360deg)}}",
      timing: "linear"
    },
    pulse: {
      frames: "@keyframes gif-pulse{0%{transform:scale(1)}50%{transform:scale(1.08)}100%{transform:scale(1)}}",
      timing: "ease-in-out"
    },
    fade: {
      frames: "@keyframes gif-fade{0%{opacity:1}50%{opacity:0.25}100%{opacity:1}}",
      timing: "ease-in-out"
    }
  };
  const item = table[animate];
  if (!item) throw new Error("unknown animate preset: " + animate);
  return item.frames + " .gif-target{animation-name:gif-" + animate +
    ";animation-duration:" + duration + "s;animation-timing-function:" + item.timing +
    ";animation-iteration-count:infinite;animation-fill-mode:both;" +
    "transform-box:fill-box;transform-origin:center;}";
}

async function main() {
  const plan = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
  const { chromium } = loadPlaywright();
  const browser = await chromium.launch({
    headless: true,
    args: ["--force-color-profile=srgb", "--hide-scrollbars"]
  });
  try {
    const context = await browser.newContext({
      viewport: { width: plan.width, height: plan.height },
      deviceScaleFactor: plan.dpr || 1,
      colorScheme: "light",
      reducedMotion: "no-preference"
    });
    const page = await context.newPage();

    if (plan.isSvg) {
      const svg = fs.readFileSync(plan.input, "utf8");
      const css = "html,body{margin:0;padding:0;width:" + plan.width + "px;height:" +
        plan.height + "px;overflow:hidden;background:" + plan.background +
        ";}svg{display:block;width:100%;height:100%;}";
      const html = "<!doctype html><html><head><meta charset=\"utf-8\">" +
        "<base href=\"" + plan.baseUrl + "\"><style>" + css + "</style></head><body>" +
        svg + "</body></html>";
      await page.setContent(html, { waitUntil: "load" });
    } else {
      await page.goto(pathToFileURL(plan.input).href, { waitUntil: "load" });
      await page.addStyleTag({
        content: "html,body{margin:0;padding:0;overflow:hidden;background:" +
          plan.background + ";}"
      });
    }

    await page.evaluate(async (config) => {
      if (config.animate && config.animate !== "none") {
        const style = document.createElement("style");
        style.textContent = config.css;
        document.head.appendChild(style);
        const el = config.selector
          ? document.querySelector(config.selector)
          : document.querySelector("svg");
        if (!el) throw new Error("selector not found: " + config.selector);
        el.classList.add("gif-target");
      }
      if (document.fonts && document.fonts.ready) await document.fonts.ready;
      await new Promise((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(resolve)));
      document.getAnimations().forEach((a) => a.pause());
      const svg = document.querySelector("svg");
      if (svg && svg.pauseAnimations) svg.pauseAnimations();
    }, {
      animate: plan.animate,
      selector: plan.selector,
      css: plan.animate && plan.animate !== "none"
        ? presetCss(plan.animate, plan.duration) : ""
    });

    for (const frame of plan.frames) {
      await page.evaluate((time) => {
        document.getAnimations().forEach((a) => {
          try { a.currentTime = time * 1000; } catch (err) { /* detached */ }
        });
        const svg = document.querySelector("svg");
        if (svg && svg.setCurrentTime) svg.setCurrentTime(time);
      }, frame.t);
      await page.screenshot({
        path: frame.path,
        animations: "allow",
        caret: "hide",
        fullPage: false
      });
    }
  } finally {
    await browser.close();
  }
}

main().catch((err) => {
  console.error("FAIL " + err.message);
  process.exit(1);
});
