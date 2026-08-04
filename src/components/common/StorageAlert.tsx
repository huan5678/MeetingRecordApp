/**
 * Boot-time banner for an unreachable storage folder (external drive not
 * plugged in). The backend falls back to the default app-data folder so the app
 * still starts; here we tell the user and let them pick a new location.
 */

import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "@/lib/tauri";
import { useSettingsStore } from "@/stores/settingsStore";
import { Button } from "@/components/common/Button";

export function StorageAlert() {
  const [deadDir, setDeadDir] = useState<string | null>(null);

  useEffect(() => {
    void api
      .getStorageUsage()
      .then((u) => setDeadDir(u.unavailableDir ?? null))
      .catch(() => setDeadDir(null));
  }, []);

  if (!deadDir) return null;

  const choose = async () => {
    const next = await open({ directory: true });
    if (typeof next === "string") {
      useSettingsStore.getState().setField("storageDir", next);
      setDeadDir(null);
    }
  };

  return (
    <div className="flex flex-wrap items-center justify-between gap-3 border-b border-line bg-amber-50 px-6 py-3 text-sm dark:bg-amber-950/40">
      <p className="text-fg">
        儲存位置「{deadDir}」目前無法存取(外接硬碟未連接?)。新錄音會暫時存到預設資料夾。
      </p>
      <Button variant="secondary" onClick={() => void choose()}>
        選擇新位置
      </Button>
    </div>
  );
}
