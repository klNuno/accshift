export interface FolderInfo {
  id: string;
  name: string;
  parentId: string | null;
  platform: string;
}

export interface ItemRef {
  type: "account" | "folder";
  id: string;
}

export interface FolderStore {
  version?: number;
  folders: FolderInfo[];
  itemOrder: Record<string, ItemRef[]>;
}
