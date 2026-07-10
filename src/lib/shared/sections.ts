import type { FolderInfo, ItemRef } from "$lib/features/folders/types";

/** One rendered section of the workspace: a folder header (null = root
 * loose items) plus the items shown under it. */
export type DisplaySection = {
  folder: FolderInfo | null;
  folderItems: ItemRef[];
  accountItems: ItemRef[];
};
