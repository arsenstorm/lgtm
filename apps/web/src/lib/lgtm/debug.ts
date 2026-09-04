/** localStorage key, "1" while the account menu's "Stretch text" is on. Dev
 *  only: a production build never mounts the observer. */
export const STRETCH_KEY = "lgtm-stretch";

const WORDS =
  "retry semantics identical regression endpoint swallowed orchestrator worktree runner checkpoint escalate diff review merged conflicted rollback transcript".split(
    " "
  );
const MIN_WORDS = 20;
const EXTRA_WORDS = 60;
const MIN_REPEATS = 4;
const EXTRA_REPEATS = 6;

/** A random replacement: a long sentence, then one very long word, so every
 *  string has to both truncate and break. */
function filler(): string {
  const words = MIN_WORDS + Math.floor(Math.random() * EXTRA_WORDS);
  const sentence = Array.from(
    { length: words },
    () => WORDS[Math.floor(Math.random() * WORDS.length)]
  ).join(" ");
  const word = Math.random()
    .toString(36)
    .slice(2)
    .repeat(MIN_REPEATS + Math.floor(Math.random() * EXTRA_REPEATS));
  return `${sentence} ${word}`;
}

/** Replaces every text node under `root` with filler and keeps doing so as
 *  React renders new text. Working on the DOM rather than the data means
 *  static labels, headings and buttons stretch too, and no id or status the
 *  code dereferences is ever touched. Returns the disconnect. */
export function stretchDom(root: Node): () => void {
  const own = new WeakMap<Text, string>();

  const replace = (text: Text) => {
    if (!text.nodeValue?.trim() || own.get(text) === text.nodeValue) {
      return;
    }
    if (text.parentElement?.closest("script,style")) {
      return;
    }
    const next = filler();
    own.set(text, next);
    text.nodeValue = next;
  };

  const stretch = (node: Node) => {
    if (node instanceof Text) {
      replace(node);
      return;
    }
    const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
    for (let text = walker.nextNode(); text; text = walker.nextNode()) {
      replace(text as Text);
    }
  };

  const observer = new MutationObserver((records) => {
    for (const record of records) {
      if (record.type === "characterData") {
        replace(record.target as Text);
      }
      for (const added of record.addedNodes) {
        stretch(added);
      }
    }
  });

  stretch(root);
  observer.observe(root, {
    characterData: true,
    childList: true,
    subtree: true,
  });
  return () => observer.disconnect();
}
