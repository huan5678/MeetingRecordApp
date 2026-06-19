/**
 * Minimal, dependency-free Markdown → HTML renderer for AI summary bodies.
 *
 * Supports only what our structured summaries use: `##`/`###` headings,
 * `-`/`*` bullet lists, `**bold**`, `*italic*`, and paragraphs. All input is
 * HTML-escaped first, so it is safe to inject the output via
 * `dangerouslySetInnerHTML` (we never emit attributes derived from input).
 *
 * This intentionally does NOT aim to be a full CommonMark implementation — it's
 * a tiny helper that keeps the bundle lean. Swap for a real renderer if the
 * summary format grows.
 */

/** Escape the five HTML-significant characters. */
export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/** Inline emphasis on already-escaped text. */
function inline(escaped: string): string {
  return escaped
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/\*([^*]+)\*/g, "<em>$1</em>");
}

export function renderBasicMarkdown(src: string): string {
  const lines = src.replace(/\r\n/g, "\n").split("\n");
  const out: string[] = [];
  let inList = false;

  const closeList = () => {
    if (inList) {
      out.push("</ul>");
      inList = false;
    }
  };

  for (const raw of lines) {
    const line = raw.trimEnd();
    const escaped = escapeHtml(line);

    if (line.trim() === "") {
      closeList();
      continue;
    }

    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      closeList();
      const level = Math.min(6, heading[1].length);
      out.push(`<h${level}>${inline(escapeHtml(heading[2]))}</h${level}>`);
      continue;
    }

    const bullet = /^[-*]\s+(.*)$/.exec(line);
    if (bullet) {
      if (!inList) {
        out.push("<ul>");
        inList = true;
      }
      out.push(`<li>${inline(escapeHtml(bullet[1]))}</li>`);
      continue;
    }

    closeList();
    out.push(`<p>${inline(escaped)}</p>`);
  }

  closeList();
  return out.join("\n");
}
