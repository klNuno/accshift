// Property inspector of "Switch account", bundled into
// dev.mtsu.accshift.sdPlugin/ui/inspector.js. It speaks the Stream Deck
// property inspector WebSocket protocol directly and asks the plugin for
// the lists: `getPlatforms` and `getAccounts`.

import { translate } from "./i18n.js";
import {
  accountOptions,
  platformOptions,
  settingsForAccount,
  settingsForPlatform,
} from "./inspector-model.js";

const state = {
  socket: null,
  context: "",
  action: "",
  language: "en",
  settings: {},
  accounts: [],
};

const $ = (id) => document.getElementById(id);

function send(message) {
  if (state.socket?.readyState === WebSocket.OPEN) state.socket.send(JSON.stringify(message));
}

function toPlugin(payload) {
  send({ event: "sendToPlugin", action: state.action, context: state.context, payload });
}

function saveSettings(settings) {
  state.settings = settings;
  send({ event: "setSettings", context: state.context, payload: settings });
}

function fill(select, options) {
  select.replaceChildren(
    ...options.map((option) => {
      const element = document.createElement("option");
      element.value = option.value;
      element.textContent = option.label;
      element.selected = option.selected;
      element.disabled = Boolean(option.disabled);
      return element;
    }),
  );
}

function showError(message) {
  const box = $("error");
  box.textContent = message;
  box.hidden = !message;
}

function loading(select) {
  fill(select, [{ value: "", label: translate(state.language, "pi.loading"), selected: true }]);
}

function requestPlatforms() {
  showError("");
  loading($("platform"));
  toPlugin({ event: "getPlatforms" });
}

function requestAccounts(platform) {
  showError("");
  state.accounts = [];
  if (!platform) {
    fill($("account"), accountOptions(state.language, [], state.settings));
    return;
  }
  loading($("account"));
  toPlugin({ event: "getAccounts", platform });
}

function onPluginMessage(payload) {
  if (payload.event === "platforms") {
    fill($("platform"), platformOptions(state.language, payload.items, state.settings.platform));
    requestAccounts(state.settings.platform);
  } else if (payload.event === "accounts" && payload.platform === state.settings.platform) {
    state.accounts = payload.items;
    fill($("account"), accountOptions(state.language, payload.items, state.settings));
  } else if (payload.event === "error") {
    showError(payload.message);
    if (payload.request === "getPlatforms") {
      fill($("platform"), platformOptions(state.language, [], state.settings.platform));
    }
    fill($("account"), accountOptions(state.language, [], state.settings));
  }
}

function translatePage() {
  for (const element of document.querySelectorAll("[data-i18n]")) {
    element.textContent = translate(state.language, element.dataset.i18n);
  }
  document.documentElement.lang = state.language;
}

function wire() {
  $("platform").addEventListener("change", (event) => {
    saveSettings(settingsForPlatform(event.target.value));
    requestAccounts(event.target.value);
  });
  $("account").addEventListener("change", (event) => {
    const account = state.accounts.find((item) => item.id === event.target.value);
    if (account) saveSettings(settingsForAccount(state.settings.platform, account));
  });
  $("refresh").addEventListener("click", requestPlatforms);
}

window.connectElgatoStreamDeckSocket = (port, uuid, registerEvent, info, actionInfo) => {
  const parsedInfo = JSON.parse(info);
  const parsedAction = JSON.parse(actionInfo);
  state.context = uuid;
  state.action = parsedAction.action;
  state.language = parsedInfo?.application?.language ?? "en";
  state.settings = parsedAction?.payload?.settings ?? {};
  translatePage();
  wire();

  state.socket = new WebSocket(`ws://127.0.0.1:${port}`);
  state.socket.addEventListener("open", () => {
    send({ event: registerEvent, uuid });
    requestPlatforms();
  });
  state.socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    if (message.event === "sendToPropertyInspector") onPluginMessage(message.payload ?? {});
    if (message.event === "didReceiveSettings") {
      state.settings = message.payload?.settings ?? {};
    }
  });
};
