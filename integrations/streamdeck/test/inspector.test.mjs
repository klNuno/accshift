// The property inspector script against a minimal fake page and socket:
// what the selects and the error box show after each plugin answer.

import assert from "node:assert/strict";
import { describe, test } from "node:test";

function fakeElement(id) {
  const listeners = {};
  return {
    textContent: "",
    hidden: id === "error",
    options: [],
    replaceChildren(...children) {
      this.options = children;
    },
    addEventListener(type, fn) {
      listeners[type] = fn;
    },
    fire(type, event) {
      listeners[type]?.(event);
    },
  };
}

let loads = 0;

/** Loads a fresh copy of src/inspector.js on a fake page and opens its socket. */
async function openInspector(settings = {}) {
  const ids = ["platform", "account", "refresh", "error"];
  const elements = Object.fromEntries(ids.map((id) => [id, fakeElement(id)]));
  const sent = [];
  let socket;
  globalThis.document = {
    getElementById: (id) => elements[id],
    createElement: () => ({}),
    querySelectorAll: () => [],
    documentElement: {},
  };
  globalThis.window = {};
  globalThis.WebSocket = class {
    static OPEN = 1;
    readyState = 1;
    listeners = {};
    constructor() {
      socket = this;
    }
    addEventListener(type, fn) {
      this.listeners[type] = fn;
    }
    send(text) {
      sent.push(JSON.parse(text));
    }
  };
  loads += 1;
  await import(`../src/inspector.js?load=${loads}`);
  window.connectElgatoStreamDeckSocket(
    1234,
    "ctx",
    "registerPropertyInspector",
    JSON.stringify({ application: { language: "en" } }),
    JSON.stringify({ action: "dev.mtsu.accshift.switch", payload: { settings } }),
  );
  socket.listeners.open();
  const fromPlugin = (payload) =>
    socket.listeners.message({ data: JSON.stringify({ event: "sendToPropertyInspector", payload }) });
  const labels = (id) => elements[id].options.map((option) => option.textContent);
  return { elements, sent, fromPlugin, labels };
}

const PLATFORMS = [
  { id: "steam", name: "Steam" },
  { id: "gog", name: "GOG Galaxy" },
];

describe("property inspector page", () => {
  test("a platform list that fails to load leaves Loading for the placeholder", async () => {
    const { elements, fromPlugin, labels } = await openInspector({ platform: "steam" });
    assert.deepEqual(labels("platform"), ["Loading"]);
    fromPlugin({ event: "error", request: "getPlatforms", message: "accshift is not installed." });
    assert.deepEqual(labels("platform"), ["Choose a platform", "steam"]);
    assert.equal(elements.error.hidden, false);
  });

  test("an account list failure keeps the platform list", async () => {
    const { fromPlugin, labels } = await openInspector({ platform: "steam" });
    fromPlugin({ event: "platforms", items: PLATFORMS });
    fromPlugin({ event: "error", request: "getAccounts", message: "accshift did not answer: x" });
    assert.deepEqual(labels("platform"), ["Choose a platform", "Steam", "GOG Galaxy"]);
  });

  test("loading accounts again clears the previous error", async () => {
    const { elements, fromPlugin, sent } = await openInspector({ platform: "steam" });
    fromPlugin({ event: "platforms", items: PLATFORMS });
    fromPlugin({ event: "error", request: "getAccounts", message: "accshift did not answer: x" });
    assert.equal(elements.error.hidden, false);

    elements.platform.fire("change", { target: { value: "gog" } });
    assert.equal(elements.error.hidden, true);
    assert.equal(elements.error.textContent, "");
    assert.deepEqual(sent.at(-1).payload, { event: "getAccounts", platform: "gog" });
  });
});
