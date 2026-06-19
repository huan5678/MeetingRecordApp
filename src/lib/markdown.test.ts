import { describe, it, expect } from "vitest";
import { escapeHtml, renderBasicMarkdown } from "@/lib/markdown";

describe("escapeHtml", () => {
  it("escapes all five significant characters", () => {
    expect(escapeHtml(`<a href="x">&'`)).toBe(
      "&lt;a href=&quot;x&quot;&gt;&amp;&#39;",
    );
  });
});

describe("renderBasicMarkdown", () => {
  it("renders headings at the right level", () => {
    expect(renderBasicMarkdown("## Overview")).toBe("<h2>Overview</h2>");
    expect(renderBasicMarkdown("### Sub")).toBe("<h3>Sub</h3>");
  });

  it("groups consecutive bullets into one list", () => {
    const html = renderBasicMarkdown("- one\n- two");
    expect(html).toBe("<ul>\n<li>one</li>\n<li>two</li>\n</ul>");
  });

  it("wraps plain lines in paragraphs", () => {
    expect(renderBasicMarkdown("hello world")).toBe("<p>hello world</p>");
  });

  it("applies bold and italic inline", () => {
    expect(renderBasicMarkdown("a **b** *c*")).toBe(
      "<p>a <strong>b</strong> <em>c</em></p>",
    );
  });

  it("escapes HTML in the source (XSS-safe)", () => {
    const html = renderBasicMarkdown("<script>alert(1)</script>");
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
  });

  it("handles a mixed document", () => {
    const md = "## Title\n\nintro line\n\n- a\n- b";
    expect(renderBasicMarkdown(md)).toBe(
      "<h2>Title</h2>\n<p>intro line</p>\n<ul>\n<li>a</li>\n<li>b</li>\n</ul>",
    );
  });
});
