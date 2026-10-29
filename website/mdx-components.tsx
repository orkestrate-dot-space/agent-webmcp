import type { MDXComponents } from "mdx/types";
import type { ReactElement, ReactNode } from "react";

import { CodeBlock } from "./app/docs/code-block";

// Fenced ```lang blocks arrive as <pre><code className="language-lang">.
// Unpack them into our tokenizer CodeBlock so MDX pages keep the same
// highlighting without depending on a syntax-highlighting runtime.
function MdxPre({ children }: { children?: ReactNode }) {
  const code = children as ReactElement<{ children?: ReactNode; className?: string }> | undefined;
  const props = code?.props;
  const lang = (props?.className ?? "").replace(/^language-/, "") || "bash";
  const text = Array.isArray(props?.children)
    ? props.children.map((c) => String(c)).join("")
    : String(props?.children ?? "");
  return <CodeBlock code={text.replace(/\n$/, "")} lang={lang} />;
}

export function useMDXComponents(components: MDXComponents): MDXComponents {
  return {
    ...components,
    pre: MdxPre,
  };
}
