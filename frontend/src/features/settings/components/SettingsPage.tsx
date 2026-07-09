import { HardwareSection } from "./HardwareSection";
import { RemoteBackendsSection } from "./RemoteBackendsSection";
import { UpdateChecker } from "../../updater/components/UpdateChecker";

/// Settings view. Hosts the Hardware and remote-backend sections plus the app
/// update check; a home for future app-level settings (theme, storage, etc.).
export function SettingsPage() {
  return (
    <div className="space-y-6" data-testid="settings">
      <HardwareSection />
      <RemoteBackendsSection />
      <UpdateChecker />
    </div>
  );
}
