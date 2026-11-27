#!/usr/bin/env node
// Styles for old Android WebViews. Runs after `vite build` (see package.json).
//
// Tailwind 4 needs a 2023 browser engine (Chrome 111): its whole output sits in
// cascade layers and uses color-mix(), oklch() and the `translate` property.
// The WebView in an Android app is only as new as the phone's "Android System
// WebView". On an old one every Tailwind rule is ignored and the app shows as
// bare HTML (seen in Google Play's review screenshots of v0.3.1).
//
// This script appends a second copy of the styles, rewritten for old engines,
// to the built stylesheet:
//
//     <the stylesheet as built, untouched>
//     @supports not (color:color-mix(in lab,red,red)) { <copy for old engines> }
//
// A current WebView skips that block, so nothing changes there. An old one
// takes the copy (its rules win over whatever it understood of the original).
//
// What the copy changes, by the Chrome version that needs it:
//   < 111  color-mix() on the app's colors  -> the same color with alpha
//          oklch() theme colors             -> hex
//          styles a library adds by script  -> included (see injectedStyles)
//   < 104  `translate`, `rotate`            -> `transform`
//   <  99  cascade layers                   -> flattened, same precedence
//   <  90  focus ring on a tapped button    -> off, as in current engines
//   <  89  ::file-selector-button in lists  -> split (one unknown selector voids a rule)
//   <  88  :where() / :is()                 -> expanded, still losing to utilities
//   <  87  inset, padding-inline, ...       -> longhands
//   <  85  @property defaults               -> set on every element
//   <  84  flex `gap`                       -> margins
//   <  80  overflow-wrap: anywhere          -> word-break: break-word
//
// The build fails if the copy still contains something an old engine cannot
// read, so a Tailwind upgrade that emits new syntax is noticed here, not on a
// user's phone.
//
//     node scripts/legacy-css.mjs [dist]

import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import postcss from "postcss";
import cascadeLayers from "@csstools/postcss-cascade-layers";
import selectorParser from "postcss-selector-parser";
import { transform } from "lightningcss";

/// The copy is written for this Chrome version and newer.
const OLDEST_CHROME = 69;
/// True in engines that do not need the copy (Chrome 111+); Tailwind's own test.
const MODERN = "(color:color-mix(in lab,red,red))";
const GUARD = `@supports not ${MODERN}`;
/// Version stand-ins: a property that arrived in the same Chrome release.
const NO_TAP_FOCUS_FIX = "not (overflow:clip)"; // Chrome < 90: a tap still draws the focus ring
const NO_WHERE = "not (aspect-ratio:1/1)"; // Chrome < 88, like :where()
// (Not `appearance`: Lightning CSS would add the -webkit- form, which old Chrome has.)
const NO_FLEX_GAP = "not (color:revert)"; // Chrome < 84, like flex gap
/// Layer for the expanded :where() rules: above the reset, below every utility.
const LOW = "utilities-low";

/// The toast library (sonner) adds its stylesheet with a script: `inject("…css…")`.
const INJECTED_CSS = /\("((?:[^"\\]|\\.)*\[data-sonner-toaster\](?:[^"\\]|\\.)*)"\)/g;
/// Other `createElement("style")` in the script: React's <style precedence>, unused here.
const OTHER_STYLE_ELEMENTS = 1;

const fail = (message) => {
  throw new Error(`legacy-css: ${message}`);
};

// ---- Steps -----------------------------------------------------------------

/// Stylesheets that a library adds with JavaScript when the app starts. In a
/// current engine those rules are outside every layer, so they win over all
/// Tailwind rules (the toast's width over `w-full`, for example). The copy needs
/// them as its own unlayered rules; otherwise its flattened Tailwind rules,
/// which carry extra weight, would win instead.
function injectedStyles(dist) {
  const dir = path.join(dist, "assets");
  const found = [];
  let created = 0;
  for (const file of fs.readdirSync(dir).filter((f) => f.endsWith(".js"))) {
    const js = fs.readFileSync(path.join(dir, file), "utf8");
    created += js.split('createElement("style")').length - 1;
    for (const m of js.matchAll(INJECTED_CSS)) found.push(JSON.parse(`"${m[1]}"`));
  }
  if (found.length !== 1 || created !== found.length + OTHER_STYLE_ELEMENTS) {
    fail(
      `the scripts create ${created} <style> element(s) and ${found.length} known stylesheet(s) were found; ` +
        `expected ${1 + OTHER_STYLE_ELEMENTS} and 1. A library that adds styles by script must be added to injectedStyles().`,
    );
  }
  return found.join("\n");
}

/// Values of the custom properties set on the root element.
function rootVariables(root) {
  const vars = new Map();
  root.walkRules((rule) => {
    if (!/^(:root|:host|:root,\s*:host)$/.test(rule.selector.trim())) return;
    rule.walkDecls(/^--/, (d) => vars.set(d.prop, d.value.trim()));
  });
  return vars;
}

/// Tailwind keeps the defaults of its --tw-* variables in @property rules and,
/// for engines without @property, in a block limited to Safari and Firefox.
/// Old Chrome needs that block too.
function variableDefaults(root) {
  let defaults = null;
  root.walkAtRules("supports", (at) => {
    if (!at.params.includes("-webkit-hyphens")) return;
    if (defaults) fail("more than one @property fallback block");
    defaults = new Set();
    at.walkDecls(/^--/, (d) => defaults.add(d.prop));
    // Used by transforms(): per element, so a rotated parent does not rotate its children.
    at.each((r) => r.append({ prop: "--tw-legacy-rotate", value: "initial" }));
    at.replaceWith(at.nodes);
  });
  if (!defaults) fail("the @property fallback block was not found");
  root.walkAtRules("property", (at) => {
    const hasInitial = at.nodes.some((d) => d.prop === "initial-value");
    if (hasInitial && !defaults.has(at.params.trim())) fail(`no default for ${at.params}`);
    at.remove();
  });
}

function hexWithAlpha(color, percent) {
  let m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(color ?? "");
  if (!m) return null;
  let hex = m[1].toLowerCase();
  if (hex.length === 3) hex = [...hex].map((c) => c + c).join("");
  const alpha = Math.round((Number(percent) / 100) * 255);
  return `#${hex}${alpha.toString(16).padStart(2, "0")}`;
}

/// `bg-[var(--npt-blue)]/10` becomes color-mix() for new engines and plain
/// `var(--npt-blue)` (no transparency) for old ones. Mixing with `transparent`
/// only sets the alpha, so the exact color can be written out instead.
function staticColorMix(root, vars) {
  root.walkAtRules("supports", (at) => {
    if (!at.params.includes("color-mix(")) return;
    const exact = [];
    for (const mixed of at.nodes) {
      if (mixed.type !== "rule") fail(`unexpected ${mixed.type} in a color-mix block`);
      // The rule new engines override; the minifier may have merged it into a selector list.
      let fallback = at.prev();
      while (fallback && !(fallback.type === "rule" && fallback.selectors.includes(mixed.selector))) fallback = fallback.prev();
      const decls = [];
      mixed.walkDecls((d) => {
        // For its own theme colors Tailwind already wrote a static fallback.
        const written = fallback?.nodes.find((n) => n.type === "decl" && n.prop === d.prop)?.value;
        if (written && !/var\(|currentcolor/i.test(written)) return;
        const m = /^color-mix\(in oklab,\s*(.+?)\s*([\d.]+)%,\s*transparent\)$/.exec(d.value.trim());
        if (!m) fail(`unexpected color-mix(): ${d.value} (${mixed.selector})`);
        const variable = /^var\((--[\w-]+)\)$/.exec(m[1]);
        // A placeholder is half of the text color; the app's text color is --npt-text.
        const placeholder = /^::?(-\w+-)?(input-)?placeholder$/.test(mixed.selector);
        const source = variable ? vars.get(variable[1]) : placeholder && /^currentcolor$/i.test(m[1]) ? vars.get("--npt-text") : m[1];
        const color = hexWithAlpha(source, m[2]);
        if (!color) fail(`cannot work out ${d.value} for ${mixed.selector}`);
        decls.push([d.prop, color]);
      });
      if (decls.length) exact.push(rule(mixed.selector, decls));
    }
    at.replaceWith(exact);
  });
}

/// `translate` and `rotate` are their own properties since Chrome 104. Older
/// engines get one `transform` that every such utility sets in full, so two of
/// them on one element still combine (in the order the properties apply).
/// Engines 104-110 read both this copy and the original rule, so the original
/// property is switched off.
const COMPOSED = "translate(var(--tw-translate-x,0),var(--tw-translate-y,0)) rotate(var(--tw-legacy-rotate,0deg))";

function transforms(root) {
  root.walkDecls(/^(translate|rotate|scale)$/, (d) => {
    const value = d.value.replace(/\s+/g, " ").trim();
    if (value === "none") return;
    if (d.prop === "rotate" && /^-?[\d.]+(deg|rad|grad|turn)$/.test(value)) {
      d.cloneBefore({ prop: "--tw-legacy-rotate", value });
    } else if (!(d.prop === "translate" && value === "var(--tw-translate-x) var(--tw-translate-y)")) {
      fail(`no rule for ${d.prop}: ${d.value} (add it to transforms())`);
    }
    d.cloneBefore({ prop: d.prop, value: "none" });
    d.prop = "transform";
    d.value = COMPOSED;
  });
  // Tailwind's `transform` utility (3D rotation and skew) must keep the rest.
  root.walkDecls("transform", (d) => {
    if (d.value.startsWith("var(--tw-rotate-x,)")) d.value = `${COMPOSED} ${d.value}`;
  });
}

const text = (nodes) => nodes.map(String).join("");

/// One selector without :where() / :is(): a list with every combination.
function expandSelector(selector) {
  const nodes = selector.nodes;
  const at = nodes.findIndex((n) => n.type === "pseudo" && /^:(where|is)$/i.test(n.value));
  if (at < 0) {
    const plain = selector.toString().trim();
    if (/:(where|is)\(/i.test(plain)) fail(`:where() inside another selector: ${plain}`);
    return [plain];
  }
  let from = at;
  while (from > 0 && nodes[from - 1].type !== "combinator") from--;
  let to = at + 1;
  while (to < nodes.length && nodes[to].type !== "combinator") to++;
  const before = nodes.slice(from, at);
  const after = nodes.slice(at + 1, to);
  const isTag = (n) => n.type === "tag" || n.type === "universal";
  const out = [];
  for (const option of nodes[at].nodes) {
    let middle;
    if (option.nodes.some((n) => n.type === "combinator")) {
      if (before.length || after.length) fail(`:where() with a combinator next to other selectors: ${selector}`);
      middle = option.toString().trim();
    } else {
      const tags = [...before, ...option.nodes].filter(isTag);
      if (tags.length > 1) fail(`two element names in one selector: ${selector}`);
      // The element name comes first in a compound selector.
      middle = text(tags) + text(before.filter((n) => !isTag(n))) + text(option.nodes.filter((n) => !isTag(n))).trim() + text(after);
    }
    const next = text(nodes.slice(0, from)) + middle + text(nodes.slice(to));
    for (const s of selectorParser().astSync(next).nodes) out.push(...expandSelector(s));
  }
  return out;
}

/// :where() and :is() arrived in Chrome 88; before that the whole rule is void.
/// A :where() rule weighs nothing, so among the utilities any class wins over
/// it (`mt-12` over the spacing of a `space-y-*` parent). The expanded rule
/// does weigh something, so it goes into a layer of its own below the
/// utilities, where it loses to them in the same way.
function expandWhere(root) {
  let count = 0;
  const low = postcss.atRule({ name: "layer", params: LOW });
  root.walkRules((r) => {
    if (!/:(where|is)\(/i.test(r.selector) || r.parent?.params === NO_WHERE) return;
    const expanded = selectorParser().astSync(r.selector).nodes.flatMap(expandSelector);
    const block = postcss.atRule({ name: "supports", params: NO_WHERE });
    block.append(r.clone({ selector: expanded.join(",") }));
    let layer = r.parent;
    while (layer && !(layer.type === "atrule" && layer.name === "layer")) layer = layer.parent;
    if (!layer || layer.params === "base") r.after(block);
    else if (layer.params === "utilities" && r.parent === layer) low.append(block);
    else fail(`a :where() rule in an unexpected place (${layer.params}): ${r.selector}`);
    count++;
  });
  if (low.nodes?.length) root.append(low);
  return count;
}

const rule = (selector, decls) => {
  const r = postcss.rule({ selector });
  for (const [prop, value] of decls) r.append({ prop, value });
  return r;
};

/// True for a selector that is exactly one class (`.gap-2`, `.mt-\[2px\]`).
function isSingleClass(selector) {
  const list = selectorParser().astSync(selector).nodes;
  return list.length === 1 && list[0].nodes.length === 1 && list[0].nodes[0].type === "class";
}

/// `gap` works in flex containers since Chrome 84 (in grids long before).
/// Before that, the same space as margins between the children. A child's own
/// margin on that side (`mt-2`) is added to the gap, as `gap` does; `auto`
/// margins stay `auto`.
function flexGap(root) {
  const utilities = root.nodes.find((n) => n.type === "atrule" && n.name === "layer" && n.params === "utilities");
  if (!utilities) fail("the utilities layer was not found");

  const gaps = [];
  const own = []; // [class, side ("mt" | "ml"), value]
  utilities.each((r) => {
    if (r.type !== "rule" || !isSingleClass(r.selector)) return;
    for (const d of r.nodes) {
      if (d.type !== "decl") continue;
      if (/^(row-gap|column-gap)$/.test(d.prop)) fail(`no flex fallback for ${d.prop} (${r.selector})`);
      if (d.prop === "gap") gaps.push([r.selector, d.value]);
      const v = parts(d.value);
      // The gap sits before a child: above it in a column, left of it in a row.
      if (/^margin(-top|-block|-block-start)?$/.test(d.prop)) own.push([r.selector, "mt", v[0]]);
      if (/^margin(-left|-inline|-inline-start)?$/.test(d.prop)) own.push([r.selector, "ml", d.prop === "margin" ? (v[3] ?? v[1] ?? v[0]) : v[0]]);
    }
  });
  if (!gaps.length) fail("no gap utilities found");

  const side = { mt: "margin-top", ml: "margin-left" };
  const block = postcss.atRule({ name: "supports", params: NO_FLEX_GAP });
  // Per element, so a child's margin is not taken for its own children's.
  block.append(rule("*,:before,:after", [["--tw-legacy-mt", "initial"], ["--tw-legacy-ml", "initial"]]));
  for (const [c, s, v] of own) if (v !== "auto") block.append(rule(c, [[`--tw-legacy-${s}`, v]]));
  // Order matters from here: these selectors weigh the same, so the later rule wins.
  for (const [c, v] of gaps) block.append(rule(`.flex${c}>*+*,.inline-flex${c}>*+*`, [["margin-left", `calc(${v} + var(--tw-legacy-ml,0px))`]]));
  for (const [c, v] of gaps) {
    block.append(rule(`.flex-col${c}>*+*`, [["margin-left", "var(--tw-legacy-ml,0px)"], ["margin-top", `calc(${v} + var(--tw-legacy-mt,0px))`]]));
  }
  for (const [c, v] of gaps) block.append(rule(`.flex-wrap${c}>*`, [["margin", `calc(${v} / 2)`]]));
  for (const [c, s, v] of own) if (v === "auto") block.append(rule(`.flex>${c},.inline-flex>${c}`, [[side[s], "auto"]]));
  utilities.append(block);

  // The app's own classes that are flex containers with a gap.
  const custom = postcss.atRule({ name: "supports", params: NO_FLEX_GAP });
  root.each((r) => {
    if (r.type !== "rule") return;
    const get = (prop) => r.nodes.find((d) => d.type === "decl" && d.prop === prop)?.value;
    const gap = get("gap");
    if (!gap || !/^(inline-)?flex$/.test(get("display") ?? "")) return;
    if (get("flex-wrap")) fail(`no gap fallback for a wrapping flex rule (${r.selector})`);
    const prop = /^column/.test(get("flex-direction") ?? "") ? "margin-top" : "margin-left";
    const selector = r.selector.split(",").map((x) => `${x.trim()}>*+*`).join(",");
    custom.append(rule(selector, [[prop, gap]]));
  });
  if (custom.nodes?.length) root.append(custom);
  return gaps.length;
}

/// Splits a value at top-level spaces: "a calc(b * c)" -> ["a", "calc(b * c)"].
function parts(value) {
  const out = [];
  let depth = 0;
  let current = "";
  for (const ch of value.trim()) {
    if (ch === "(") depth++;
    if (ch === ")") depth--;
    if (/\s/.test(ch) && depth === 0) {
      if (current) out.push(current);
      current = "";
    } else current += ch;
  }
  if (current) out.push(current);
  return out;
}

/// inset and the two-sided logical shorthands arrived in Chrome 87. Lightning
/// CSS lowers them, but not when the value is a var() expression, which is how
/// Tailwind writes them. The app has no right-to-left language, so the inline
/// axis of `inset` is left/right; padding and margin keep their logical sides
/// (those longhands are old enough).
function shorthands(root) {
  const sides = {
    inset: ["top", "right", "bottom", "left"],
    "inset-block": ["top", "bottom"],
    "inset-inline": ["left", "right"],
    "padding-block": ["padding-top", "padding-bottom"],
    "padding-inline": ["padding-inline-start", "padding-inline-end"],
    "margin-block": ["margin-top", "margin-bottom"],
    "margin-inline": ["margin-inline-start", "margin-inline-end"],
  };
  root.walkDecls((d) => {
    if (/^(border|scroll-margin|scroll-padding)-(inline|block)(-width|-style|-color)?$/.test(d.prop)) {
      fail(`no rule for ${d.prop} (add it to shorthands())`);
    }
    const names = sides[d.prop];
    if (!names) return;
    const v = parts(d.value);
    if (v.length < 1 || v.length > names.length) fail(`unexpected ${d.prop}: ${d.value}`);
    // CSS shorthand order: 1 value = all, 2 = first/second pair, 3 = top, sides, bottom.
    const pick = names.length === 2 ? [v[0], v[1] ?? v[0]] : [v[0], v[1] ?? v[0], v[2] ?? v[0], v[3] ?? v[1] ?? v[0]];
    names.forEach((prop, i) => d.cloneBefore({ prop, value: pick[i] }));
    d.remove();
  });
}

/// Before Chrome 90 a tap leaves the focus ring on a button (orange on
/// Android). Current engines draw it for keyboard focus only. In the lowest
/// layer, so a focus style the app sets itself still wins.
function tapFocus(root) {
  const block = postcss.atRule({ name: "supports", params: NO_TAP_FOCUS_FIX });
  block.append(rule('button:focus,a:focus,[role="button"]:focus', [["outline", "none"]]));
  const base = postcss.atRule({ name: "layer", params: "base" });
  base.append(block);
  root.append(base);
}

/// Fewer layers and no ID selectors keep the flattened selectors short: the
/// layer polyfill repeats `:not(#\#)` once per layer below, times the highest
/// ID count in the file plus one. (`#root` becomes `[id="root"]`: it weighs
/// less, which is fine for the one rule that sizes html, body and #root.)
function simplifyLayers(root) {
  root.walkAtRules("layer", (at) => {
    if (!at.nodes) return at.remove(); // an order statement such as `@layer components;`
    // Variable defaults, theme variables and the reset never set the same property.
    if (/^(properties|theme)$/.test(at.params)) at.params = "base";
    else if (![LOW, "base", "utilities"].includes(at.params)) fail(`unexpected layer: ${at.params}`);
  });
  root.prepend(postcss.atRule({ name: "layer", params: `base, ${LOW}, utilities` }));
  root.walkRules((r) => {
    if (!r.selector.includes("#")) return;
    r.selector = selectorParser((list) => {
      list.walkIds((id) => id.replaceWith(selectorParser.attribute({ attribute: "id", operator: "=", value: id.value, quoteMark: '"', raws: {} })));
    }).processSync(r.selector);
  });
}

/// overflow-wrap: anywhere arrived in Chrome 80; word-break: break-word is the
/// older spelling of the same behavior.
function wordBreak(root) {
  root.walkDecls("overflow-wrap", (d) => {
    if (d.value.trim() === "anywhere") d.cloneBefore({ prop: "word-break", value: "break-word" });
  });
}

/// Anything an engine older than Chrome 111 would drop must be gone by now.
function check(css) {
  const body = css.split(`@supports ${NO_WHERE}`).join("").split(`@supports ${NO_FLEX_GAP}`).join("");
  const forbidden = [
    ["a cascade layer", /@layer\b/],
    ["@property", /@property\b/],
    ["color-mix()", /color-mix\(/],
    ["oklch() / oklab()", /\bokl(ch|ab)\(/],
    ["an individual transform", /[;{](translate:(?!none)|rotate:(?!none)|scale:(?!none))/],
    ["the inset shorthand", /[;{]inset(-inline|-block)?:/],
    ["a logical shorthand", /[;{](padding|margin|border)-(inline|block):/],
    ["CSS nesting", /[;{}]\s*&/],
    [":has()", /:has\(/],
    ["a container query", /@container\b/],
    ["dynamic viewport units", /\d(dvh|svh|lvh|dvw|svw|lvw)\b/],
    ["a range media query", /@media[^{]*[<>]/],
    ["text-wrap", /[;{]text-wrap/],
    ["aspect-ratio", /[;{]aspect-ratio:/],
    ["a relative color", /\(from /],
  ];
  for (const [what, pattern] of forbidden) {
    const m = pattern.exec(body);
    if (m) fail(`the copy for old WebViews still has ${what}: …${body.slice(Math.max(0, m.index - 80), m.index + 80)}…`);
  }
  // A few rules that must have survived (utilities carry the layer polyfill's `:not(#\#)`).
  for (const anchor of [/\.flex(:not\(#\\#\))+\{display:flex\}/, /box-sizing:border-box/, /--tw-translate-x:0/]) {
    if (!anchor.test(css)) fail(`the copy for old WebViews lost ${anchor}`);
  }
}

// ---- Build -------------------------------------------------------------------

export async function legacyCopy(modernCss, injectedCss = "", name = "styles.css") {
  const root = postcss.parse(modernCss, { from: undefined });
  const vars = rootVariables(root);
  variableDefaults(root);
  staticColorMix(root, vars);
  // After the app's own rules, as in the page: the script adds them to the end of <head>.
  root.append(postcss.parse(injectedCss, { from: undefined }).nodes);
  transforms(root);
  wordBreak(root);
  const whereRules = expandWhere(root);
  const gapRules = flexGap(root);
  shorthands(root);
  tapFocus(root);
  simplifyLayers(root);

  const flat = await postcss([cascadeLayers()]).process(root, { from: undefined });
  const warnings = flat.warnings().map(String);
  if (warnings.length) fail(`cascade layers: ${warnings.join("; ")}`);

  const lowered = transform({
    filename: name,
    code: Buffer.from(flat.css),
    minify: true,
    targets: { chrome: OLDEST_CHROME << 16 },
  });
  const css = lowered.code.toString();
  check(css);
  return { css, whereRules, gapRules };
}

async function main() {
  const dist = path.resolve(process.argv[2] ?? "dist");
  const htmlPath = path.join(dist, "index.html");
  let html = fs.readFileSync(htmlPath, "utf8");
  const links = [...html.matchAll(/<link[^>]+rel="stylesheet"[^>]*>/g)].map((m) => m[0]);
  if (links.length !== 1) fail(`expected one stylesheet in index.html, found ${links.length}`);
  const href = /href="([^"]+)"/.exec(links[0])[1];
  const cssPath = path.join(dist, href);

  let modern = fs.readFileSync(cssPath, "utf8");
  const already = modern.indexOf(GUARD);
  if (already >= 0) modern = modern.slice(0, already); // run twice = same result
  if (!modern.includes("@layer")) fail("the stylesheet has no cascade layers; is this still Tailwind 4 output?");

  const { css, whereRules, gapRules } = await legacyCopy(modern, injectedStyles(dist), path.basename(cssPath));
  const out = `${modern}${GUARD}{${css}}`;

  // New content, new file name: a WebView must never pair a cached old file with a new app.
  const hash = createHash("sha256").update(out).digest("base64url").slice(0, 8);
  const newHref = href.replace(/-[\w-]{8}(\.legacy)?\.css$/, "") + `-${hash}.legacy.css`;
  // Only index.html may name the stylesheet (a script would, if the build ever split the CSS).
  for (const file of fs.readdirSync(path.dirname(cssPath)).filter((f) => f.endsWith(".js"))) {
    if (fs.readFileSync(path.join(path.dirname(cssPath), file), "utf8").includes(path.basename(href))) {
      fail(`${file} refers to ${path.basename(href)}; renaming the stylesheet would break that`);
    }
  }
  fs.rmSync(cssPath);
  fs.writeFileSync(path.join(dist, newHref), out);
  fs.writeFileSync(htmlPath, html.replace(href, newHref));

  const kb = (s) => `${(Buffer.byteLength(s) / 1024).toFixed(1)} kB`;
  console.log(
    `legacy-css: ${newHref}  ${kb(modern)} as built + ${kb(css)} for WebViews older than Chrome 111 ` +
      `(${whereRules} :where() rules expanded, ${gapRules} gap sizes as margins)`,
  );
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(new URL(import.meta.url).pathname)) {
  main().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });
}
