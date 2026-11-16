// Stream Deck entry point: wires the SDK to the SwitchAccount action.
// Bundled into dev.mtsu.accshift.sdPlugin/bin/plugin.js by scripts/build.mjs.

import streamDeck, { SingletonAction } from "@elgato/streamdeck";
import { Accshift, nodeIo } from "./accshift.js";
import { ACTION_UUID, SwitchAccount } from "./switch-account.js";

const logger = streamDeck.logger.createScope("accshift");
const log = {
  info: (message) => logger.info(message),
  warn: (message) => logger.warn(message),
  error: (message) => logger.error(message),
};

const language = () => {
  try {
    return streamDeck.info.application.language;
  } catch {
    return "en";
  }
};

const controller = new SwitchAccount(new Accshift(nodeIo(), log), { language });

class SwitchAccountAction extends SingletonAction {
  manifestId = ACTION_UUID;

  async onWillAppear(ev) {
    await controller.render(ev.action, ev.payload.settings);
  }

  async onDidReceiveSettings(ev) {
    await controller.render(ev.action, ev.payload.settings);
  }

  async onKeyDown(ev) {
    await controller.keyDown(ev.action, ev.payload.settings);
  }

  async onSendToPlugin(ev) {
    await controller.message(ev.payload, (payload) =>
      streamDeck.ui.sendToPropertyInspector(payload),
    );
  }
}

streamDeck.actions.registerAction(new SwitchAccountAction());
streamDeck.connect();
