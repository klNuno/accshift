// Every string the plugin shows, in the seven languages of the accshift app
// (crates/accshift-app/i18n). Stream Deck itself speaks de, en, es, fr, ja,
// ko, zh_CN and zh_TW: its language picks a table here, English otherwise.

export const LOCALES = ["en", "fr", "es", "pt", "pt-br", "ru", "zh"];

export const STRINGS = {
  en: {
    "plugin.description": "Switch game accounts with accshift, one key per account.",
    "action.name": "Switch account",
    "action.tooltip": "Switch to the chosen account in accshift.",
    "pi.platform": "Platform",
    "pi.account": "Account",
    "pi.choosePlatform": "Choose a platform",
    "pi.chooseAccount": "Choose an account",
    "pi.loading": "Loading",
    "pi.noAccounts": "No accounts on this platform.",
    "pi.refresh": "Refresh",
    "pi.signedIn": "{name} (signed in)",
    "pi.hint": "The key opens the switch in accshift, which keeps its PIN and its confirmations.",
    "error.notInstalled": "accshift is not installed. Get it from github.com/klNuno/accshift.",
    "error.cliDisabled":
      "Turn on \"Allow the accshift CLI\" in accshift: Settings > General > Integrations.",
    "error.failed": "accshift did not answer: {message}",
    "key.notInstalled": "No\naccshift",
  },
  fr: {
    "plugin.description": "Change de compte de jeu avec accshift, une touche par compte.",
    "action.name": "Changer de compte",
    "action.tooltip": "Passe au compte choisi dans accshift.",
    "pi.platform": "Plateforme",
    "pi.account": "Compte",
    "pi.choosePlatform": "Choisir une plateforme",
    "pi.chooseAccount": "Choisir un compte",
    "pi.loading": "Chargement",
    "pi.noAccounts": "Aucun compte sur cette plateforme.",
    "pi.refresh": "Actualiser",
    "pi.signedIn": "{name} (connecté)",
    "pi.hint":
      "La touche ouvre le changement dans accshift, qui garde son code PIN et ses confirmations.",
    "error.notInstalled": "accshift n'est pas installé. Téléchargez-le sur github.com/klNuno/accshift.",
    "error.cliDisabled":
      "Activez « Autoriser le CLI accshift » dans accshift : Paramètres > Général > Intégrations.",
    "error.failed": "accshift n'a pas répondu : {message}",
    "key.notInstalled": "Pas\nd'accshift",
  },
  es: {
    "plugin.description": "Cambia de cuenta de juego con accshift, una tecla por cuenta.",
    "action.name": "Cambiar de cuenta",
    "action.tooltip": "Cambia a la cuenta elegida en accshift.",
    "pi.platform": "Plataforma",
    "pi.account": "Cuenta",
    "pi.choosePlatform": "Elige una plataforma",
    "pi.chooseAccount": "Elige una cuenta",
    "pi.loading": "Cargando",
    "pi.noAccounts": "No hay cuentas en esta plataforma.",
    "pi.refresh": "Actualizar",
    "pi.signedIn": "{name} (conectada)",
    "pi.hint": "La tecla abre el cambio en accshift, que mantiene su PIN y sus confirmaciones.",
    "error.notInstalled": "accshift no está instalado. Descárgalo en github.com/klNuno/accshift.",
    "error.cliDisabled":
      "Activa \"Permitir la CLI de accshift\" en accshift: Configuración > General > Integraciones.",
    "error.failed": "accshift no respondió: {message}",
    "key.notInstalled": "Sin\naccshift",
  },
  pt: {
    "plugin.description": "Muda de conta de jogo com o accshift, uma tecla por conta.",
    "action.name": "Mudar de conta",
    "action.tooltip": "Muda para a conta escolhida no accshift.",
    "pi.platform": "Plataforma",
    "pi.account": "Conta",
    "pi.choosePlatform": "Escolha uma plataforma",
    "pi.chooseAccount": "Escolha uma conta",
    "pi.loading": "A carregar",
    "pi.noAccounts": "Nenhuma conta nesta plataforma.",
    "pi.refresh": "Atualizar",
    "pi.signedIn": "{name} (com sessão iniciada)",
    "pi.hint": "A tecla abre a mudança no accshift, que mantém o PIN e as confirmações.",
    "error.notInstalled": "O accshift não está instalado. Transfira-o em github.com/klNuno/accshift.",
    "error.cliDisabled":
      "Ative \"Permitir a CLI do accshift\" no accshift: Definições > Geral > Integrações.",
    "error.failed": "O accshift não respondeu: {message}",
    "key.notInstalled": "Sem\naccshift",
  },
  "pt-br": {
    "plugin.description": "Troque de conta de jogo com o accshift, uma tecla por conta.",
    "action.name": "Trocar de conta",
    "action.tooltip": "Troca para a conta escolhida no accshift.",
    "pi.platform": "Plataforma",
    "pi.account": "Conta",
    "pi.choosePlatform": "Escolha uma plataforma",
    "pi.chooseAccount": "Escolha uma conta",
    "pi.loading": "Carregando",
    "pi.noAccounts": "Nenhuma conta nesta plataforma.",
    "pi.refresh": "Atualizar",
    "pi.signedIn": "{name} (conectada)",
    "pi.hint": "A tecla abre a troca no accshift, que mantém o PIN e as confirmações.",
    "error.notInstalled": "O accshift não está instalado. Baixe em github.com/klNuno/accshift.",
    "error.cliDisabled":
      "Ative \"Permitir a CLI do accshift\" no accshift: Configurações > Geral > Integrações.",
    "error.failed": "O accshift não respondeu: {message}",
    "key.notInstalled": "Sem\naccshift",
  },
  ru: {
    "plugin.description": "Переключение игровых аккаунтов через accshift, одна клавиша на аккаунт.",
    "action.name": "Сменить аккаунт",
    "action.tooltip": "Переключает на выбранный аккаунт в accshift.",
    "pi.platform": "Платформа",
    "pi.account": "Аккаунт",
    "pi.choosePlatform": "Выберите платформу",
    "pi.chooseAccount": "Выберите аккаунт",
    "pi.loading": "Загрузка",
    "pi.noAccounts": "На этой платформе нет аккаунтов.",
    "pi.refresh": "Обновить",
    "pi.signedIn": "{name} (вход выполнен)",
    "pi.hint": "Клавиша открывает переключение в accshift, где сохраняются PIN-код и подтверждения.",
    "error.notInstalled": "accshift не установлен. Скачайте его на github.com/klNuno/accshift.",
    "error.cliDisabled":
      "Включите «Разрешить CLI accshift» в accshift: Настройки > Общие > Интеграции.",
    "error.failed": "accshift не ответил: {message}",
    "key.notInstalled": "Нет\naccshift",
  },
  zh: {
    "plugin.description": "用 accshift 切换游戏账号，每个账号一个按键。",
    "action.name": "切换账号",
    "action.tooltip": "在 accshift 中切换到所选账号。",
    "pi.platform": "平台",
    "pi.account": "账号",
    "pi.choosePlatform": "选择平台",
    "pi.chooseAccount": "选择账号",
    "pi.loading": "加载中",
    "pi.noAccounts": "此平台没有账号。",
    "pi.refresh": "刷新",
    "pi.signedIn": "{name}（已登录）",
    "pi.hint": "按键会在 accshift 中打开切换，PIN 和确认步骤依然有效。",
    "error.notInstalled": "未安装 accshift。请从 github.com/klNuno/accshift 下载。",
    "error.cliDisabled": "请在 accshift 中开启“允许 accshift 命令行工具”：设置 > 通用 > 集成。",
    "error.failed": "accshift 没有响应：{message}",
    "key.notInstalled": "未安装\naccshift",
  },
};

/**
 * The table for a Stream Deck or browser language tag.
 * @param {string | undefined} language e.g. "fr", "zh_CN", "pt-BR"
 * @returns {string}
 */
export function localeFor(language) {
  const tag = String(language ?? "")
    .toLowerCase()
    .replace(/_/g, "-");
  if (tag === "pt-br") return "pt-br";
  const base = tag.split("-")[0];
  return LOCALES.includes(base) ? base : "en";
}

/**
 * A string in `language`, English when the table lacks it, the key last.
 * `{name}` placeholders are filled from `vars`.
 */
export function translate(language, key, vars = {}) {
  const table = STRINGS[localeFor(language)] ?? STRINGS.en;
  const template = table[key] ?? STRINGS.en[key] ?? key;
  return template.replace(/\{(\w+)\}/g, (whole, name) =>
    Object.hasOwn(vars, name) ? String(vars[name]) : whole,
  );
}

/** Stream Deck locale files (`<lang>.json` at the plugin root) and the table each one uses. */
export const STREAM_DECK_LOCALES = { en: "en", fr: "fr", es: "es", zh_CN: "zh" };

/**
 * The content of one Stream Deck locale file: the manifest strings, and the
 * whole table under `Localization` for `streamDeck.i18n`.
 * @param {string} locale a STRINGS key
 * @param {string} actionUuid
 */
export function streamDeckLocale(locale, actionUuid) {
  const table = { ...STRINGS.en, ...STRINGS[locale] };
  return {
    Description: table["plugin.description"],
    Name: "accshift",
    [actionUuid]: { Name: table["action.name"], Tooltip: table["action.tooltip"] },
    Localization: table,
  };
}
