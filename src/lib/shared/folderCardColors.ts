import { CLIENT_STORE_FOLDER_CARD_COLORS } from "$lib/storage/clientStorage";
import { createCachedMapStore } from "./cachedMapStore";

const SAFE_COLOR_RE = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/;

const store = createCachedMapStore(CLIENT_STORE_FOLDER_CARD_COLORS, (_key, color) =>
  SAFE_COLOR_RE.test(color) ? color : null,
);

export const getFolderCardColor = store.get;
export const setFolderCardColor = store.set;
