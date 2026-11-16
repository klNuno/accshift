// The "Switch account" action, without the Stream Deck SDK: the plugin
// entry point hands it the SDK's action objects, the tests hand it fakes.
//
// Settings of a key: { platform, accountId, accountName, avatarPath }.

import { errorKey } from "./accshift.js";
import { CliError } from "./cli-output.js";
import { translate } from "./i18n.js";
import { keyImage } from "./key-image.js";

export const ACTION_UUID = "dev.mtsu.accshift.switch";

/** How long the "not installed" title stays on the key. */
export const NOTICE_MS = 4000;

export class SwitchAccount {
  /**
   * @param {import("./accshift.js").Accshift} accshift
   * @param {{ language: () => string, setTimeout?: typeof setTimeout }} options
   */
  constructor(accshift, options) {
    this.accshift = accshift;
    this.language = options.language;
    this.setTimeout = options.setTimeout ?? setTimeout;
  }

  /** Title and image of a key from its settings. */
  async render(action, settings = {}) {
    await action.setTitle(settings.accountName ?? "");
    await action.setImage(
      settings.platform ? await keyImage(settings, this.accshift.io) : undefined,
    );
  }

  async keyDown(action, settings = {}) {
    try {
      await this.accshift.switchTo(settings);
      await action.showOk();
    } catch (error) {
      await action.showAlert();
      if (error instanceof CliError && error.code === "not_installed") {
        await action.setTitle(translate(this.language(), "key.notInstalled"));
        this.setTimeout(() => {
          action.setTitle(settings.accountName ?? "");
        }, NOTICE_MS);
      } else if (!(error instanceof CliError)) {
        this.accshift.log.error(`key press failed: ${error?.message ?? error}`);
      }
    }
  }

  /**
   * A message of the property inspector; the answer goes through `send`.
   * `getPlatforms` answers `platforms`, `getAccounts` answers `accounts`,
   * a failure answers `error` with a translated message.
   */
  async message(payload, send) {
    const event = payload?.event;
    try {
      if (event === "getPlatforms") {
        await send({ event: "platforms", items: await this.accshift.platforms() });
      } else if (event === "getAccounts" && typeof payload.platform === "string") {
        const items = await this.accshift.accounts(payload.platform);
        await send({ event: "accounts", platform: payload.platform, items });
      }
    } catch (error) {
      const key = errorKey(error);
      if (key === "error.failed") {
        // A CliError's message comes from the CLI process: log its code only.
        const reason =
          error instanceof CliError
            ? `${error.code}, exit code ${error.exitCode ?? "none"}`
            : (error?.message ?? error);
        this.accshift.log.error(`accshift CLI ${event} failed: ${reason}`);
      }
      await send({
        event: "error",
        request: event,
        message: translate(this.language(), key, { message: error?.message ?? String(error) }),
      });
    }
  }
}
