// SPDX-License-Identifier: GPL-3.0-or-later
// The news column. The body of a post comes from Rust as blocks of text runs (news_html.rs); it
// goes into the page as text only, never as HTML. A link opens in the browser through Rust,
// which checks it again.

const { invoke } = window.__TAURI__.core;

/** Text shown on the toggle of a closed and an opened post. */
const READ_MORE = "Read more";
const SHOW_LESS = "Show less";
/** Indent of a list item per level of nesting, in pixels. */
const ITEM_INDENT = 18;
/** The line break inside a text run. */
const LINE_BREAK = "\n";

/** Post ids the player opened in this launch. */
const opened = new Set();
/** Picture addresses already fetched, by post id. */
const pictures = new Map();

/** One text run as nodes: its style tags around its text, and its link. */
function spanNode(span, onError) {
  let node = document.createDocumentFragment();
  span.text.split(LINE_BREAK).forEach((part, index) => {
    if (index > 0) node.append(document.createElement("br"));
    node.append(document.createTextNode(part));
  });
  for (const [flag, tag] of [["code", "code"], ["emphasis", "em"], ["strong", "strong"]]) {
    if (span.style[flag]) {
      const wrapper = document.createElement(tag);
      wrapper.append(node);
      node = wrapper;
    }
  }
  if (span.link) {
    const link = document.createElement("a");
    link.href = span.link;
    link.append(node);
    link.addEventListener("click", (event) => {
      event.preventDefault();
      invoke("open_link", { url: span.link }).catch(onError);
    });
    // A middle or other button must not open the link inside the launcher window.
    link.addEventListener("auxclick", (event) => event.preventDefault());
    node = link;
  }
  return node;
}

/** The blocks of a post body as nodes. */
export function renderBlocks(blocks, onError) {
  const body = document.createElement("div");
  body.className = "body";
  for (const block of blocks) {
    const element = document.createElement(block.kind.type === "item" ? "div" : "p");
    if (block.kind.type === "item") {
      element.className = "item";
      element.style.paddingLeft = `${block.kind.depth * ITEM_INDENT}px`;
      const marker = document.createElement("span");
      marker.textContent = block.kind.marker;
      element.append(marker);
      const text = document.createElement("span");
      block.spans.forEach((span) => text.append(spanNode(span, onError)));
      element.append(text);
    } else {
      block.spans.forEach((span) => element.append(spanNode(span, onError)));
    }
    body.append(element);
  }
  return body;
}

/** Shows the cached picture of `post` in `image`. */
function showPicture(post, image) {
  const known = pictures.get(post.id);
  if (known) {
    image.src = known;
    return;
  }
  invoke("news_picture", { id: post.id }).then((address) => {
    if (!address) return;
    pictures.set(post.id, address);
    image.src = address;
    image.hidden = false;
  });
}

/** One post. */
function postNode(post, rerender, onError) {
  const isOpen = opened.has(post.id);
  const article = document.createElement("article");
  article.className = isOpen ? "post open" : "post";
  if (post.pinned) article.classList.add("pinned");

  const meta = document.createElement("div");
  meta.className = "post-meta";
  const badge = document.createElement("span");
  badge.className = post.downtime ? "badge downtime" : "badge";
  badge.textContent = post.kind;
  const date = document.createElement("span");
  date.className = "meta";
  date.textContent = post.date;
  meta.append(badge, date);

  const title = document.createElement("h3");
  title.textContent = post.title;
  article.append(meta, title);

  if (post.hasPicture) {
    const image = document.createElement("img");
    image.className = "post-picture";
    image.alt = "";
    image.hidden = !pictures.has(post.id);
    showPicture(post, image);
    article.append(image);
  }

  if (isOpen) {
    article.append(renderBlocks(post.blocks, onError));
  } else {
    const preview = document.createElement("p");
    preview.className = "preview";
    preview.textContent = post.preview;
    article.append(preview);
  }

  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "link-button";
  toggle.textContent = isOpen ? SHOW_LESS : READ_MORE;
  toggle.addEventListener("click", () => {
    if (isOpen) opened.delete(post.id);
    else opened.add(post.id);
    rerender();
  });
  article.append(toggle);
  return article;
}

/** Forgets the fetched pictures: fresh news may change the picture of a post. */
export function forgetPictures() {
  pictures.clear();
}

/** Shows `news` (the NewsView of news.rs) in `status` and `posts`. */
export function renderNews(news, status, posts, onError) {
  status.hidden = !news.statusLine;
  status.textContent = news.statusLine ?? "";
  const rerender = () => renderNews(news, status, posts, onError);
  posts.replaceChildren(...news.posts.map((post) => postNode(post, rerender, onError)));
}
