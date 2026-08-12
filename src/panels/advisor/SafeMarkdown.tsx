// src/panels/advisor/SafeMarkdown.tsx
//
// SafeMarkdown — a minimal, XSS-safe Markdown renderer for the AI
// Advisor result view.
//
// WHY NOT dangerouslySetInnerHTML / a raw-HTML markdown lib:
// project rule + spec both forbid `dangerouslySetInnerHTML`. The advisor
// renders untrusted LLM output, so we NEVER turn model text into HTML.
// Instead we tokenise a safe subset of GitHub-flavoured Markdown
// (headings, bullet/numbered lists, paragraphs, inline `code`, **bold**,
// *italic*) into React elements. Anything we don't recognise is rendered
// as plain text — it can never become markup.
//
// (If the project later adds `react-markdown`, this component can be
// swapped for it 1:1 — same "render markdown, never inject HTML"
// contract. Kept dependency-free here so the AV-clean Free build adds no
// new npm package.)

import React from "react";

// ── Inline formatting: `code`, **bold**, *italic* ──────────────────────
// Splits on the first matching delimiter run; recurses on the remainder.
function renderInline(text: string, keyPrefix: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  // Order matters: code first (so we don't format inside it), then bold,
  // then italic.
  const pattern = /(`[^`]+`|\*\*[^*]+\*\*|\*[^*]+\*)/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  let i = 0;
  while ((match = pattern.exec(text)) !== null) {
    if (match.index > lastIndex) {
      nodes.push(text.slice(lastIndex, match.index));
    }
    const token = match[0];
    const key = `${keyPrefix}-${i++}`;
    if (token.startsWith("`")) {
      nodes.push(
        <code key={key} className="advisor-md-code">
          {token.slice(1, -1)}
        </code>,
      );
    } else if (token.startsWith("**")) {
      nodes.push(<strong key={key}>{token.slice(2, -2)}</strong>);
    } else {
      nodes.push(<em key={key}>{token.slice(1, -1)}</em>);
    }
    lastIndex = match.index + token.length;
  }
  if (lastIndex < text.length) {
    nodes.push(text.slice(lastIndex));
  }
  return nodes;
}

interface SafeMarkdownProps {
  source: string;
}

export default function SafeMarkdown({ source }: SafeMarkdownProps) {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const blocks: React.ReactNode[] = [];

  let i = 0;
  let blockKey = 0;
  while (i < lines.length) {
    const line = lines[i];
    const trimmed = line.trim();

    // Blank line → block separator.
    if (trimmed === "") {
      i++;
      continue;
    }

    // Headings: #, ##, ###
    const heading = /^(#{1,6})\s+(.*)$/.exec(trimmed);
    if (heading) {
      const level = Math.min(heading[1].length, 6);
      const content = renderInline(heading[2], `h-${blockKey}`);
      const props = { key: `b-${blockKey++}`, className: `advisor-md-h${level}` };
      blocks.push(React.createElement(`h${level}`, props, content));
      i++;
      continue;
    }

    // Unordered list: -, *, +
    if (/^[-*+]\s+/.test(trimmed)) {
      const items: React.ReactNode[] = [];
      let li = 0;
      while (i < lines.length && /^[-*+]\s+/.test(lines[i].trim())) {
        const itemText = lines[i].trim().replace(/^[-*+]\s+/, "");
        items.push(<li key={`li-${blockKey}-${li}`}>{renderInline(itemText, `uli-${blockKey}-${li}`)}</li>);
        li++;
        i++;
      }
      blocks.push(
        <ul key={`b-${blockKey++}`} className="advisor-md-ul">
          {items}
        </ul>,
      );
      continue;
    }

    // Ordered list: 1. 2. 3.
    if (/^\d+\.\s+/.test(trimmed)) {
      const items: React.ReactNode[] = [];
      let li = 0;
      while (i < lines.length && /^\d+\.\s+/.test(lines[i].trim())) {
        const itemText = lines[i].trim().replace(/^\d+\.\s+/, "");
        items.push(<li key={`oli-${blockKey}-${li}`}>{renderInline(itemText, `ooli-${blockKey}-${li}`)}</li>);
        li++;
        i++;
      }
      blocks.push(
        <ol key={`b-${blockKey++}`} className="advisor-md-ol">
          {items}
        </ol>,
      );
      continue;
    }

    // Paragraph: gather consecutive non-blank, non-structural lines.
    const para: string[] = [];
    while (
      i < lines.length &&
      lines[i].trim() !== "" &&
      !/^(#{1,6})\s+/.test(lines[i].trim()) &&
      !/^[-*+]\s+/.test(lines[i].trim()) &&
      !/^\d+\.\s+/.test(lines[i].trim())
    ) {
      para.push(lines[i].trim());
      i++;
    }
    blocks.push(
      <p key={`b-${blockKey++}`} className="advisor-md-p">
        {renderInline(para.join(" "), `p-${blockKey}`)}
      </p>,
    );
  }

  return <div className="advisor-md">{blocks}</div>;
}
