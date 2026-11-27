#!/usr/bin/env node
// Text directly inside a flex container that has a `gap-*` class.
//
// Android WebViews older than Chrome 84 have no `gap` for flex containers.
// For them scripts/legacy-css.mjs turns the gap into margins between the
// children, and a margin needs an element: `<Icon /> Copy` gets no space,
// `<Icon /><span>Copy</span>` does. Current engines draw both the same.
//
// Lists every such place; exits 1 if there is one.
//
//     node scripts/check-flex-gap.mjs [src]

import fs from "node:fs";
import path from "node:path";
import ts from "typescript";

const root = path.resolve(process.argv[2] ?? "src");
const files = [];
(function walk(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p);
    else if (p.endsWith(".tsx")) files.push(p);
  }
})(root);

/// The static text of a className attribute (string, template or expression).
const classText = (attr) => (attr?.initializer ? attr.initializer.getText() : "");

/// True when a child expression can render as bare text.
function rendersText(expr) {
  if (!expr) return false;
  if (ts.isParenthesizedExpression(expr)) return rendersText(expr.expression);
  if (ts.isStringLiteralLike(expr) || ts.isTemplateExpression(expr) || ts.isNumericLiteral(expr)) return true;
  if (ts.isConditionalExpression(expr)) return rendersText(expr.whenTrue) || rendersText(expr.whenFalse);
  if (ts.isBinaryExpression(expr)) {
    const op = expr.operatorToken.kind;
    if (op === ts.SyntaxKind.AmpersandAmpersandToken) return rendersText(expr.right);
    if (op === ts.SyntaxKind.BarBarToken || op === ts.SyntaxKind.QuestionQuestionToken) return rendersText(expr.left) || rendersText(expr.right);
    if (op === ts.SyntaxKind.PlusToken) return true;
    return false;
  }
  if (ts.isJsxElement(expr) || ts.isJsxSelfClosingElement(expr) || ts.isJsxFragment(expr)) return false;
  // t("…"), format helpers, plain variables and property reads: text unless named like an element.
  const text = expr.getText();
  if (/^(children|icon|\w*Icon|\w*Node|\w*Element|right|main|rows|\w+\.map\()/.test(text) || /\.map\(/.test(text)) return false;
  return true;
}

const found = [];
for (const file of files) {
  const source = ts.createSourceFile(file, fs.readFileSync(file, "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  (function visit(node) {
    if (ts.isJsxElement(node)) {
      const attrs = node.openingElement.attributes.properties;
      const cls = classText(attrs.find((a) => ts.isJsxAttribute(a) && a.name.getText() === "className"));
      if (/(^|[\s"'`])gap-[\w.[\]/-]+/.test(cls) && /(^|[\s"'`])(inline-)?flex([\s"'`]|$)/.test(cls)) {
        // One child alone needs no gap.
        const children = node.children.filter((c) => !(ts.isJsxText(c) && !c.getText().trim()));
        for (const child of children) {
          const bare = ts.isJsxText(child) ? child.getText().trim() : ts.isJsxExpression(child) && rendersText(child.expression) ? child.getText() : null;
          if (bare && children.length > 1) {
            const { line } = source.getLineAndCharacterOfPosition(child.getStart());
            found.push(`${path.relative(process.cwd(), file)}:${line + 1}  ${bare.replace(/\s+/g, " ").slice(0, 70)}`);
          }
        }
      }
    }
    ts.forEachChild(node, visit);
  })(source);
}

if (found.length) {
  console.error("Text directly inside a flex container with a gap (wrap it in <span>…</span>):");
  for (const f of found) console.error("  " + f);
  process.exit(1);
}
console.log(`check-flex-gap: ${files.length} files, no bare text in flex containers with a gap`);
