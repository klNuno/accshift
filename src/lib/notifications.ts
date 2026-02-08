const STORAGE_KEY = "zazaswitcher_notifications";

export interface AppNotification {
  id: string;
  message: string;
  timestamp: number;
}

function load(): AppNotification[] {
  try {
    const data = localStorage.getItem(STORAGE_KEY);
    return data ? JSON.parse(data) : [];
  } catch {
    return [];
  }
}

function save(notifs: AppNotification[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(notifs));
}

export function getNotifications(): AppNotification[] {
  return load();
}

export function addNotification(message: string) {
  const notifs = load();
  notifs.unshift({
    id: Date.now().toString(),
    message,
    timestamp: Date.now(),
  });
  save(notifs);
}

export function clearNotifications() {
  save([]);
}

export function getUnreadCount(): number {
  return load().length;
}
