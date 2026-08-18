import { type FormEvent, type ReactNode, useEffect, useRef, useState } from "react";
import {
  Bell,
  Code2,
  Check,
  Download,
  Edit3,
  EyeOff,
  History,
  Image,
  KeyRound,
  Keyboard,
  Link,
  LogOut,
  MessageCircle,
  Monitor,
  RefreshCcw,
  RotateCcw,
  Search,
  ShieldAlert,
  ShieldCheck,
  ShieldQuestion,
  ShieldX,
  SlidersHorizontal,
  Smartphone,
  Upload,
  UserRound,
  X
} from "lucide-react";

import { t } from "../i18n/messages";
import { ImeSafeForm, ImeTextField, SecureImeTextField } from "./ImeTextControl";
import { KeyboardSettingsContent } from "./KeyboardSettingsPanel";
import { SearchHistorySection } from "./user-settings/SearchHistorySection";
import { AccountManagementSection } from "./user-settings/AccountManagementSection";
import { SessionsSection } from "./user-settings/SessionsSection";
import {
  DetailRow,
  TrustActionButton,
  TrustStatusRow,
  failureKindLabel,
  type TrustTone
} from "./user-settings/SettingsStatusPrimitives";
import { TrustHelpButton } from "./TrustHelp";
import type { DisplayDensity } from "../app/localPresentation";
import type { ShortcutLabelProfile } from "../domain/shortcuts";
import { mediaSourceUrl } from "../domain/mediaUrl";
import type {
  AccountManagementCapabilities,
  AccountManagementState,
  CrossSigningStatus,
  DeviceSessionListState,
  DeviceTrustLevel,
  DisplaySettings,
  E2eeTrustState,
  EmojiPreference,
  DisplayPlatform,
  FontPreference,
  IdentityResetState,
  KeyBackupStatus,
  LocalEncryptionState,
  NotificationSettings,
  RecoveryKeyDeliveryState,
  RoomSummary,
  SavedSessionInfo,
  SearchCrawlerState,
  SettingsPatch,
  SettingsState,
  ProfileState,
  RoomKeyExportState,
  RoomKeyImportState,
  SecureBackupPassphraseChangeState,
  SecureBackupSetupState,
  ThemePreference,
  TimelineSettings,
  VerificationFlowState
} from "../domain/types";

export function UserSettingsPanel({
  currentSession,
  displayDensity = "comfortable",
  savedSessions,
  settings,
  searchCrawlerState,
  profile,
  e2eeTrust,
  localEncryption,
  platform,
  deviceSessions,
  accountManagement,
  accountManagementCapabilities,
  keyboardLabelProfile,
  onUpdateSettings,
  onRebuildSearchIndex,
  onSetDisplayName,
  onSetAvatar,
  onBootstrapCrossSigning,
  onEnableKeyBackup,
  onChooseRoomKeyExportDestination,
  onChooseRoomKeyImportSource,
  onChooseSecureBackupDestination = async () => null,
  onExportRoomKeys,
  onImportRoomKeys,
  onBootstrapSecureBackup,
  onChangeSecureBackupPassphrase,
  onAcceptVerification,
  onConfirmSasVerification,
  onCancelVerification,
  onResetIdentity,
  onCancelIdentityReset,
  onSubmitIdentityResetPassword,
  onSubmitIdentityResetOAuth,
  onProbeLocalEncryption,
  onResetLocalData,
  onLogout,
  onOpenRecovery,
  onSwitchAccount,
  onQueryDevices,
  onRenameDevice,
  onDeleteDevices,
  onLoadAccountManagementCapabilities,
  onChangePassword,
  onDeactivateAccount,
  onSubmitAccountManagementUia,
  onStartCrawlRoom,
  onStopCrawlRoom,
  onDisplayDensityChange = () => undefined,
  accountManagementUrl = null,
  onManageAccount = () => undefined,
  rooms
}: {
  currentSession: SavedSessionInfo | null;
  displayDensity?: DisplayDensity;
  savedSessions: SavedSessionInfo[];
  settings: SettingsState;
  searchCrawlerState?: SearchCrawlerState;
  profile: ProfileState;
  e2eeTrust: E2eeTrustState;
  localEncryption: LocalEncryptionState;
  platform: DisplayPlatform;
  deviceSessions: DeviceSessionListState;
  accountManagement: AccountManagementState;
  accountManagementCapabilities: AccountManagementCapabilities;
  keyboardLabelProfile?: ShortcutLabelProfile;
  onOpenKeyboardSettings: () => void;
  onUpdateSettings: (patch: SettingsPatch) => void;
  onRebuildSearchIndex?: () => void;
  onSetDisplayName: (displayName: string | null) => void;
  onSetAvatar: (file: File) => void;
  onBootstrapCrossSigning: () => void;
  onEnableKeyBackup: () => void;
  onChooseRoomKeyExportDestination: () => Promise<string | null>;
  onChooseRoomKeyImportSource: () => Promise<string | null>;
  onChooseSecureBackupDestination?: () => Promise<string | null>;
  onExportRoomKeys: (destinationPath: string, passphrase: string) => void;
  onImportRoomKeys: (sourcePath: string, passphrase: string) => void;
  onBootstrapSecureBackup: (
    passphrase: string | null,
    recoveryKeyDestinationPath: string | null
  ) => void;
  onChangeSecureBackupPassphrase: (
    oldSecret: string,
    newPassphrase: string,
    recoveryKeyDestinationPath: string | null
  ) => void;
  onAcceptVerification: (flowId: number) => void;
  onConfirmSasVerification: (flowId: number) => void;
  onCancelVerification: (flowId: number) => void;
  onResetIdentity: () => void;
  onCancelIdentityReset: (flowId: number) => void;
  onSubmitIdentityResetPassword: (flowId: number, password: string) => void;
  onSubmitIdentityResetOAuth: (flowId: number) => void;
  onProbeLocalEncryption: () => void;
  onResetLocalData: () => void;
  onLogout: () => void;
  onOpenRecovery: () => void;
  onSwitchAccount: (session: SavedSessionInfo) => void;
  onQueryDevices: () => void;
  onRenameDevice: (deviceOrdinal: number, displayName: string) => void;
  onDeleteDevices: (deviceOrdinals: number[]) => void;
  onLoadAccountManagementCapabilities: () => void;
  onChangePassword: (newPassword: string) => void;
  onDeactivateAccount: (eraseData: boolean) => void;
  onSubmitAccountManagementUia: (flowId: number, password: string) => void;
  onStartCrawlRoom?: (roomId: string) => void;
  onStopCrawlRoom?: (roomId: string) => void;
  onDisplayDensityChange?: (density: DisplayDensity) => void;
  accountManagementUrl?: string | null;
  onManageAccount?: () => void;
  rooms?: RoomSummary[];
}) {
  useEffect(() => {
    if (deviceSessions.kind === "idle" && currentSession) {
      onQueryDevices();
    }
  }, [deviceSessions.kind, currentSession, onQueryDevices]);
  const selectedTheme = settings.values.appearance.theme;
  const selectedFont = settings.values.typography.font;
  const selectedEmoji = settings.values.typography.emoji;
  const selectedTimeline = settings.values.timeline;
  const selectedNotifications = settings.values.notifications;
  const selectedDisplay = settings.values.display;
  const isSaving = settings.persistence.kind === "saving";
  const [displayNameDraft, setDisplayNameDraft] = useState(profile.own.display_name ?? "");
  const panelRef = useRef<HTMLElement | null>(null);
  const avatarInputRef = useRef<HTMLInputElement | null>(null);
  const profileBusy = profile.update.kind !== "idle";
  const displayNameBusy = profile.update.kind === "settingDisplayName";
  const avatarBusy = profile.update.kind === "settingAvatar";
  const profileAvatarUrl = avatarSourceUrl(profile.own.avatar);
  const profileInitial = profile.own.display_name?.charAt(0).toUpperCase()
    || accountInitial(currentSession?.user_id ?? "");

  useEffect(() => {
    setDisplayNameDraft(profile.own.display_name ?? "");
  }, [profile.own.display_name]);

  function submitDisplayName(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (profileBusy) {
      return;
    }
    const trimmed = displayNameDraft.trim();
    onSetDisplayName(trimmed.length > 0 ? trimmed : null);
  }

  function selectAvatarFile(file: File | null) {
    if (!file || avatarBusy) {
      return;
    }
    onSetAvatar(file);
  }

  function scrollToSection(sectionId: string) {
    panelRef.current
      ?.querySelector<HTMLElement>(`#${sectionId}`)
      ?.scrollIntoView({ block: "start" });
  }

  return (
    <section
      ref={panelRef}
      className="settings-panel user-settings-panel"
      aria-labelledby="user-settings-title"
    >
      <header className="settings-panel-header">
        <div>
          <h2 id="user-settings-title">{t("panel.userSettings")}</h2>
          <p dir="auto">{currentSession?.user_id ?? t("settings.matrixAccount")}</p>
        </div>
      </header>

      <div className="settings-list">
        <button
          className="settings-list-item"
          type="button"
          onClick={() => scrollToSection("settings-general")}
        >
          <span className="settings-list-label">
            <span className="settings-list-icon" aria-hidden="true">
              <UserRound size={16} />
            </span>
            <span>{t("settings.general")}</span>
          </span>
        </button>
        <button
          className="settings-list-item"
          type="button"
          onClick={() => scrollToSection("settings-appearance")}
        >
          <span className="settings-list-label">
            <span className="settings-list-icon" aria-hidden="true">
              <SlidersHorizontal size={16} />
            </span>
            <span>{t("settings.appearance")}</span>
          </span>
        </button>
        <button
          className="settings-list-item"
          type="button"
          onClick={() => scrollToSection("settings-display")}
        >
          <span className="settings-list-label">
            <span className="settings-list-icon" aria-hidden="true">
              <Monitor size={16} />
            </span>
            <span>{t("settings.display")}</span>
          </span>
        </button>
        <button
          className="settings-list-item"
          type="button"
          onClick={() => scrollToSection("settings-notifications")}
        >
          <span className="settings-list-label">
            <span className="settings-list-icon" aria-hidden="true">
              <Bell size={16} />
            </span>
            <span>{t("settings.notifications")}</span>
          </span>
        </button>
        <button
          className="settings-list-item"
          type="button"
          onClick={() => scrollToSection("settings-messaging-privacy")}
        >
          <span className="settings-list-label">
            <span className="settings-list-icon" aria-hidden="true">
              <MessageCircle size={16} />
            </span>
            <span>{t("settings.messagingPrivacy")}</span>
          </span>
        </button>
        <button
          className="settings-list-item"
          type="button"
          onClick={() => scrollToSection("settings-keyboard")}
        >
          <span className="settings-list-label">
            <span className="settings-list-icon" aria-hidden="true">
              <Keyboard size={16} />
            </span>
            <span>{t("settings.keyboard")}</span>
          </span>
        </button>
        <button
          className="settings-list-item"
          type="button"
          onClick={() => scrollToSection("settings-timeline")}
        >
          <span className="settings-list-label">
            <span className="settings-list-icon" aria-hidden="true">
              <History size={16} />
            </span>
            <span>{t("settings.timeline")}</span>
          </span>
        </button>
        <button
          className="settings-list-item"
          type="button"
          onClick={() => scrollToSection("settings-search-history")}
        >
          <span className="settings-list-label">
            <span className="settings-list-icon" aria-hidden="true">
              <Search size={16} />
            </span>
            <span>{t("settings.searchHistory")}</span>
          </span>
        </button>
        <button
          className="settings-list-item"
          type="button"
          onClick={() => scrollToSection("settings-security")}
        >
          <span className="settings-list-label">
            <span className="settings-list-icon" aria-hidden="true">
              <ShieldCheck size={16} />
            </span>
            <span>{t("settings.securityPrivacy")}</span>
          </span>
        </button>
        <button
          className="settings-list-item"
          type="button"
          onClick={() => scrollToSection("settings-sessions")}
        >
          <span className="settings-list-label">
            <span className="settings-list-icon" aria-hidden="true">
              <Smartphone size={16} />
            </span>
            <span>{t("settings.sessions")}</span>
          </span>
        </button>
      </div>

      <section id="settings-general" className="settings-section" aria-label={t("settings.profile")}>
        <h3>{t("settings.profile")}</h3>
        <div className="profile-settings">
          <div className="profile-settings-avatar" aria-hidden="true">
            {profileAvatarUrl ? (
              <img src={profileAvatarUrl} />
            ) : (
              <span>{profileInitial}</span>
            )}
          </div>
          <ImeSafeForm className="profile-settings-form" onSubmit={submitDisplayName}>
            <label className="profile-settings-field">
              <span>{t("settings.profileDisplayName")}</span>
              <ImeTextField
                value={displayNameDraft}
                syncKey={currentSession?.user_id ?? "profile-display-name"}
                placeholder={t("settings.profileDisplayNamePlaceholder")}
                disabled={profileBusy}
                onChange={(event) => setDisplayNameDraft(event.currentTarget.value)}
              />
            </label>
            <div className="profile-settings-actions">
              <button
                className="profile-settings-action"
                type="submit"
                disabled={profileBusy}
              >
                <Check size={14} />
                <span>
                  {displayNameBusy ? t("settings.profileSavingDisplayName") : t("settings.profileUpdate")}
                </span>
              </button>
              <input
                ref={avatarInputRef}
                className="sr-only"
                type="file"
                accept="image/png,image/jpeg,image/webp,image/gif"
                onChange={(event) => {
                  selectAvatarFile(event.currentTarget.files?.[0] ?? null);
                  event.currentTarget.value = "";
                }}
              />
              <button
                className="profile-settings-action"
                type="button"
                disabled={profileBusy}
                onClick={() => avatarInputRef.current?.click()}
              >
                <Image size={14} />
                <span>
                  {avatarBusy ? t("settings.profileSavingAvatar") : t("settings.profileUploadAvatar")}
                </span>
              </button>
            </div>
          </ImeSafeForm>
        </div>
      </section>

      <section className="settings-section" aria-label={t("settings.session")}>
        <h3>{t("settings.session")}</h3>
        <div className="settings-detail-list">
          <DetailRow label={t("settings.homeserver")} value={currentSession?.homeserver ?? t("settings.notRestored")} />
          <DetailRow label={t("settings.userId")} value={currentSession?.user_id ?? t("settings.notRestored")} />
          <DetailRow label={t("settings.device")} value={currentSession?.device_id ?? t("settings.notRestored")} />
          <DetailRow label={t("settings.localStoreLabel")} value={t("settings.localStore")} />
        </div>
        <div className="profile-settings-actions">
          <button
            className="profile-settings-action"
            type="button"
            disabled={!currentSession}
            onClick={onLogout}
          >
            <LogOut size={14} />
            <span>{t("settings.signOut")}</span>
          </button>
        </div>
      </section>

      <section id="settings-keyboard" className="settings-section" aria-label={t("settings.keyboard")}>
        <div className="settings-section-heading">
          <div>
            <h3>{t("settings.keyboard")}</h3>
            <p>{t("settings.keyboardDescription")}</p>
          </div>
          {isSaving ? <span className="settings-save-state">{t("settings.saving")}</span> : null}
        </div>
        <KeyboardSettingsContent
          isSaving={isSaving}
          labelProfile={keyboardLabelProfile}
          selectedSendShortcut={settings.values.keyboard.composer_send_shortcut}
          onUpdateSettings={onUpdateSettings}
        />
      </section>

      <section id="settings-timeline" className="settings-section" aria-label={t("settings.timeline")}>
        <div className="settings-section-heading">
          <h3>{t("settings.timeline")}</h3>
          {isSaving ? <span className="settings-save-state">{t("settings.saving")}</span> : null}
        </div>
        <div className="settings-toggle-list">
          <TimelineToggle
            label={t("settings.autoLoadOlderMessages")}
            description={t("settings.autoLoadOlderMessagesDescription")}
            settingKey="auto_load_older_messages"
            current={selectedTimeline}
            onSelect={onUpdateSettings}
          />
          <TimelineThreadRootOrderToggle
            label={t("settings.threadRootLatestReply")}
            description={t("settings.threadRootLatestReplyDescription")}
            current={selectedTimeline}
            onSelect={onUpdateSettings}
          />
        </div>
      </section>

      <section id="settings-appearance" className="settings-section" aria-label={t("settings.appearance")}>
        <div className="settings-section-heading">
          <h3>{t("settings.appearance")}</h3>
          {isSaving ? <span className="settings-save-state">{t("settings.saving")}</span> : null}
        </div>
        <div className="segmented-control" role="group" aria-label={t("settings.theme")}>
          <ThemeButton
            label={t("settings.themeSystem")}
            selected={selectedTheme === "system"}
            value="system"
            onSelect={onUpdateSettings}
          />
          <ThemeButton
            label={t("settings.themeLight")}
            selected={selectedTheme === "light"}
            value="light"
            onSelect={onUpdateSettings}
          />
          <ThemeButton
            label={t("settings.themeDark")}
            selected={selectedTheme === "dark"}
            value="dark"
            onSelect={onUpdateSettings}
          />
        </div>
        <div className="settings-control-row">
          <span>{t("settings.displayDensity")}</span>
          <div className="segmented-control" role="group" aria-label={t("settings.displayDensity")}>
            <DensityButton
              label={t("settings.densityCompact")}
              selected={displayDensity === "compact"}
              value="compact"
              onSelect={onDisplayDensityChange}
            />
            <DensityButton
              label={t("settings.densityDefault")}
              selected={displayDensity === "default"}
              value="default"
              onSelect={onDisplayDensityChange}
            />
            <DensityButton
              label={t("settings.densityComfortable")}
              selected={displayDensity === "comfortable"}
              value="comfortable"
              onSelect={onDisplayDensityChange}
            />
          </div>
        </div>
        <h4 className="settings-subheading">{t("settings.typography")}</h4>
        <div className="settings-control-stack">
          <div className="settings-control-row">
            <span>{t("settings.uiFont")}</span>
            <div className="segmented-control" role="group" aria-label={t("settings.uiFont")}>
              <FontButton
                label={t("settings.fontSystem")}
                selected={selectedFont === "system"}
                value="system"
                currentEmoji={selectedEmoji}
                onSelect={onUpdateSettings}
              />
              <FontButton
                label={t("settings.fontInter")}
                selected={selectedFont === "inter"}
                value="inter"
                currentEmoji={selectedEmoji}
                onSelect={onUpdateSettings}
              />
            </div>
          </div>
          <div className="settings-control-row">
            <span>{t("settings.emojiFont")}</span>
            <div className="segmented-control" role="group" aria-label={t("settings.emojiFont")}>
              <EmojiButton
                label={t("settings.fontSystem")}
                selected={selectedEmoji === "system"}
                value="system"
                currentFont={selectedFont}
                onSelect={onUpdateSettings}
              />
              <EmojiButton
                label={t("settings.twemojiColr")}
                selected={selectedEmoji === "twemojiColr"}
                value="twemojiColr"
                currentFont={selectedFont}
                onSelect={onUpdateSettings}
              />
            </div>
          </div>
        </div>
      </section>

      <section id="settings-display" className="settings-section" aria-label={t("settings.display")}>
        <div className="settings-section-heading">
          <h3>{t("settings.display")}</h3>
          {isSaving ? <span className="settings-save-state">{t("settings.saving")}</span> : null}
        </div>
        <div className="settings-toggle-list">
          <DisplayToggle
            label={t("settings.codeBlockWrap")}
            settingKey="code_block_wrap"
            icon="code"
            current={selectedDisplay}
            onSelect={onUpdateSettings}
          />
          <DisplayToggle
            label={t("settings.urlPreviewsUnencrypted")}
            description={t("settings.urlPreviewsUnencryptedDescription")}
            settingKey="url_previews_enabled"
            icon="link"
            current={selectedDisplay}
            onSelect={onUpdateSettings}
          />
          <DisplayToggle
            label={t("settings.urlPreviewsEncrypted")}
            description={t("settings.urlPreviewsEncryptedDescription")}
            settingKey="encrypted_url_previews_enabled"
            icon="link"
            current={selectedDisplay}
            onSelect={onUpdateSettings}
          />
          <DisplayToggle
            label={t("settings.hideRedacted")}
            settingKey="hide_redacted"
            icon="hideRedacted"
            current={selectedDisplay}
            onSelect={onUpdateSettings}
          />
        </div>
      </section>

      <section id="settings-notifications" className="settings-section" aria-label={t("settings.notifications")}>
        <div className="settings-section-heading">
          <h3>{t("settings.notifications")}</h3>
          {isSaving ? <span className="settings-save-state">{t("settings.saving")}</span> : null}
        </div>
        <div className="settings-toggle-list">
          <NotificationSettingToggle
            label={t("settings.notificationDesktop")}
            settingKey="desktop_notifications"
            current={selectedNotifications}
            onSelect={onUpdateSettings}
            icon={<Bell size={15} aria-hidden="true" />}
          />
          <NotificationSettingToggle
            label={t("settings.notificationSound")}
            settingKey="sound"
            current={selectedNotifications}
            onSelect={onUpdateSettings}
            icon={<Bell size={15} aria-hidden="true" />}
          />
          <NotificationSettingToggle
            label={t("settings.notificationBadges")}
            settingKey="badges"
            current={selectedNotifications}
            onSelect={onUpdateSettings}
            icon={<Bell size={15} aria-hidden="true" />}
          />
        </div>
      </section>

      <section
        id="settings-messaging-privacy"
        className="settings-section"
        aria-label={t("settings.messagingPrivacy")}
      >
        <div className="settings-section-heading">
          <h3>{t("settings.messagingPrivacy")}</h3>
          {isSaving ? <span className="settings-save-state">{t("settings.saving")}</span> : null}
        </div>
        <div className="settings-toggle-list">
          <NotificationSettingToggle
            label={t("settings.sendReadReceipts")}
            settingKey="send_read_receipts"
            current={selectedNotifications}
            onSelect={onUpdateSettings}
            icon={<Check size={15} aria-hidden="true" />}
          />
          <NotificationSettingToggle
            label={t("settings.sendTypingNotifications")}
            settingKey="send_typing_notifications"
            current={selectedNotifications}
            onSelect={onUpdateSettings}
            icon={<Edit3 size={15} aria-hidden="true" />}
          />
        </div>
      </section>

      <section
        id="settings-search-history"
        className="settings-section"
        aria-label={t("settings.searchHistory")}
      >
        <div className="settings-section-heading">
          <h3>{t("settings.searchHistory")}</h3>
          {isSaving ? <span className="settings-save-state">{t("settings.saving")}</span> : null}
        </div>
          <SearchHistorySection
            crawlerSettings={settings.values.search_crawler}
            crawlerState={searchCrawlerState ?? { rooms: {}, last_active: null }}
          rooms={rooms}
          isSaving={isSaving}
          onUpdateSettings={onUpdateSettings}
          onRebuildSearchIndex={onRebuildSearchIndex}
          onStartCrawlRoom={onStartCrawlRoom}
          onStopCrawlRoom={onStopCrawlRoom}
        />
      </section>

      <section id="settings-security" className="settings-section" aria-label={t("settings.security")}>
        <h3>{t("settings.security")}</h3>
        <SecuritySection
          keyManagement={e2eeTrust.key_management}
          localEncryption={localEncryption}
          platform={platform}
          onBootstrapSecureBackup={onBootstrapSecureBackup}
          onChangeSecureBackupPassphrase={onChangeSecureBackupPassphrase}
          onChooseRoomKeyExportDestination={onChooseRoomKeyExportDestination}
          onChooseRoomKeyImportSource={onChooseRoomKeyImportSource}
          onChooseSecureBackupDestination={onChooseSecureBackupDestination}
          onExportRoomKeys={onExportRoomKeys}
          onImportRoomKeys={onImportRoomKeys}
          onOpenRecovery={onOpenRecovery}
          onProbeLocalEncryption={onProbeLocalEncryption}
          onResetLocalData={onResetLocalData}
        />
      </section>

      <SessionsSection
        deviceSessions={deviceSessions}
        accountManagement={accountManagement}
        onQueryDevices={onQueryDevices}
        onRenameDevice={onRenameDevice}
        onDeleteDevices={onDeleteDevices}
        onSubmitAccountManagementUia={onSubmitAccountManagementUia}
      />

      <AccountManagementSection
        accountManagement={accountManagement}
        accountManagementCapabilities={accountManagementCapabilities}
        accountManagementUrl={accountManagementUrl}
        currentSession={currentSession}
        onLoadAccountManagementCapabilities={onLoadAccountManagementCapabilities}
        onChangePassword={onChangePassword}
        onDeactivateAccount={onDeactivateAccount}
        onManageAccount={onManageAccount}
        onSubmitAccountManagementUia={onSubmitAccountManagementUia}
      />

      <TrustSection
        trust={e2eeTrust}
        onAcceptVerification={onAcceptVerification}
        onBootstrapCrossSigning={onBootstrapCrossSigning}
        onCancelVerification={onCancelVerification}
        onConfirmSasVerification={onConfirmSasVerification}
        onEnableKeyBackup={onEnableKeyBackup}
        onResetIdentity={onResetIdentity}
        onCancelIdentityReset={onCancelIdentityReset}
        onSubmitIdentityResetOAuth={onSubmitIdentityResetOAuth}
        onSubmitIdentityResetPassword={onSubmitIdentityResetPassword}
      />

      {savedSessions.length > 0 ? (
        <section className="account-switcher" aria-label={t("settings.accountSwitcher")}>
          <h3>{t("settings.accounts")}</h3>
          <div className="account-switcher-list">
            {savedSessions.map((session) => {
              const isCurrent = sessionMatches(currentSession, session);
              return (
                <article className="account-switcher-row" key={sessionKey(session)}>
                  <div className="account-switcher-avatar" aria-hidden="true">
                    {accountInitial(session.user_id)}
                  </div>
                  <div className="account-switcher-main">
                    <div className="account-switcher-user" dir="auto">{session.user_id}</div>
                    <div className="account-switcher-meta" dir="auto">
                      {session.homeserver} / {session.device_id}
                    </div>
                  </div>
                  <button
                    className="account-switcher-action"
                    type="button"
                    disabled={isCurrent}
                    onClick={() => onSwitchAccount(session)}
                  >
                    <RefreshCcw size={14} />
                    <span>{isCurrent ? t("settings.current") : t("settings.switch")}</span>
                  </button>
                </article>
              );
            })}
          </div>
        </section>
      ) : null}
    </section>
  );
}

function SecuritySection({
  keyManagement,
  localEncryption,
  platform,
  onExportRoomKeys,
  onImportRoomKeys,
  onChooseRoomKeyExportDestination,
  onChooseRoomKeyImportSource,
  onChooseSecureBackupDestination,
  onBootstrapSecureBackup,
  onChangeSecureBackupPassphrase,
  onOpenRecovery,
  onProbeLocalEncryption,
  onResetLocalData
}: {
  keyManagement: E2eeTrustState["key_management"];
  localEncryption: LocalEncryptionState;
  platform: DisplayPlatform;
  onExportRoomKeys: (destinationPath: string, passphrase: string) => void;
  onImportRoomKeys: (sourcePath: string, passphrase: string) => void;
  onChooseRoomKeyExportDestination: () => Promise<string | null>;
  onChooseRoomKeyImportSource: () => Promise<string | null>;
  onChooseSecureBackupDestination: () => Promise<string | null>;
  onBootstrapSecureBackup: (
    passphrase: string | null,
    recoveryKeyDestinationPath: string | null
  ) => void;
  onChangeSecureBackupPassphrase: (
    oldSecret: string,
    newPassphrase: string,
    recoveryKeyDestinationPath: string | null
  ) => void;
  onOpenRecovery: () => void;
  onProbeLocalEncryption: () => void;
  onResetLocalData: () => void;
}) {
  const status = localEncryptionStatus(localEncryption);
  const roomKeyPassphraseRef = useRef<HTMLInputElement>(null);
  const secureBackupPassphraseRef = useRef<HTMLInputElement>(null);
  const oldSecureBackupSecretRef = useRef<HTMLInputElement>(null);
  const newSecureBackupPassphraseRef = useRef<HTMLInputElement>(null);
  const [roomKeyPassphraseRequest, setRoomKeyPassphraseRequest] =
    useState<RoomKeyPassphraseRequest | null>(null);
  async function chooseRoomKeyExport(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const destinationPath = await onChooseRoomKeyExportDestination();
    if (!destinationPath) {
      return;
    }
    setRoomKeyPassphraseRequest({ kind: "export", path: destinationPath });
  }

  async function chooseRoomKeyImport(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const sourcePath = await onChooseRoomKeyImportSource();
    if (!sourcePath) {
      return;
    }
    setRoomKeyPassphraseRequest({ kind: "import", path: sourcePath });
  }

  function submitRoomKeyPassphrase(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const passphrase = roomKeyPassphraseRef.current?.value ?? "";
    if (!roomKeyPassphraseRequest || !passphrase) {
      return;
    }
    if (roomKeyPassphraseRequest.kind === "export") {
      onExportRoomKeys(roomKeyPassphraseRequest.path, passphrase);
    } else {
      onImportRoomKeys(roomKeyPassphraseRequest.path, passphrase);
    }
    if (roomKeyPassphraseRef.current) {
      roomKeyPassphraseRef.current.value = "";
    }
    setRoomKeyPassphraseRequest(null);
  }

  function closeRoomKeyPassphraseDialog() {
    if (roomKeyPassphraseRef.current) {
      roomKeyPassphraseRef.current.value = "";
    }
    setRoomKeyPassphraseRequest(null);
  }

  async function submitSecureBackupSetup(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const passphrase = secureBackupPassphraseRef.current?.value ?? "";
    const recoveryPath = await onChooseSecureBackupDestination();
    if (!recoveryPath) {
      return;
    }
    onBootstrapSecureBackup(passphrase.length > 0 ? passphrase : null, recoveryPath);
    if (secureBackupPassphraseRef.current) {
      secureBackupPassphraseRef.current.value = "";
    }
  }

  async function submitSecureBackupPassphraseChange(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const oldSecret = oldSecureBackupSecretRef.current?.value ?? "";
    const newPassphrase = newSecureBackupPassphraseRef.current?.value ?? "";
    if (!oldSecret || !newPassphrase) {
      return;
    }
    const recoveryPath = await onChooseSecureBackupDestination();
    if (!recoveryPath) {
      return;
    }
    onChangeSecureBackupPassphrase(oldSecret, newPassphrase, recoveryPath);
    if (oldSecureBackupSecretRef.current) {
      oldSecureBackupSecretRef.current.value = "";
    }
    if (newSecureBackupPassphraseRef.current) {
      newSecureBackupPassphraseRef.current.value = "";
    }
  }

  return (
    <>
      <div className="settings-detail-list">
        <DetailRow
          label={t("settings.credentialStore")}
          value={credentialStoreLabel(platform)}
        />
        <DetailRow label={t("settings.searchIndex")} value={t("settings.searchIndex")} />
      </div>
      <div className="trust-status-list">
        <TrustStatusRow
          icon={status.icon}
          label={t("settings.localEncryption")}
          value={status.label}
          tone={status.tone}
          action={
            <TrustActionButton
              icon={<RefreshCcw size={14} />}
              label={t("settings.checkLocalEncryption")}
              disabled={localEncryption.kind === "probing"}
              onClick={onProbeLocalEncryption}
            />
          }
        />
        <TrustStatusRow
          icon={<RotateCcw size={16} />}
          label={t("settings.localData")}
          value={t("settings.localDataResetAvailable")}
          tone={localEncryption.kind === "resetting" ? "progress" : "danger"}
          action={
            <>
              <TrustActionButton
                icon={<KeyRound size={14} />}
                label={t("settings.openRecovery")}
                variant="secondary"
                onClick={onOpenRecovery}
              />
              <TrustActionButton
                icon={<RotateCcw size={14} />}
                label={t("settings.resetLocalData")}
                disabled={localEncryption.kind === "resetting"}
                onClick={onResetLocalData}
              />
            </>
          }
        />
      </div>
      <section className="settings-section" aria-label={t("settings.keyManagement")}>
        <h4 className="settings-subheading">{t("settings.keyManagement")}</h4>
        <div className="settings-control-stack">
          <ImeSafeForm
            aria-label={t("settings.roomKeyExport")}
            className="profile-settings-form"
            onSubmit={(event) => {
              void chooseRoomKeyExport(event);
            }}
          >
            <KeyManagementStatus
              label={t("settings.roomKeyExport")}
              value={roomKeyExportStatusLabel(keyManagement.room_key_export)}
              testId="room-key-export-state"
            />
            <p className="profile-settings-hint">{t("settings.chooseRoomKeyExportFile")}</p>
            <div className="profile-settings-actions">
              <button
                className="trust-action-button primary"
                type="submit"
                disabled={keyManagement.room_key_export.kind === "exporting"}
              >
                <Download size={14} />
                <span>{t("settings.exportRoomKeys")}</span>
              </button>
            </div>
          </ImeSafeForm>

          <ImeSafeForm
            aria-label={t("settings.roomKeyImport")}
            className="profile-settings-form"
            onSubmit={(event) => {
              void chooseRoomKeyImport(event);
            }}
          >
            <KeyManagementStatus
              label={t("settings.roomKeyImport")}
              value={roomKeyImportStatusLabel(keyManagement.room_key_import)}
              testId="room-key-import-state"
            />
            <p className="profile-settings-hint">{t("settings.chooseRoomKeyImportFile")}</p>
            <div className="profile-settings-actions">
              <button
                className="trust-action-button primary"
                type="submit"
                disabled={keyManagement.room_key_import.kind === "importing"}
              >
                <Upload size={14} />
                <span>{t("settings.importRoomKeys")}</span>
              </button>
            </div>
          </ImeSafeForm>

          <ImeSafeForm
            aria-label={t("settings.secureBackup")}
            className="profile-settings-form"
            onSubmit={submitSecureBackupSetup}
          >
            <KeyManagementStatus
              label={t("settings.secureBackup")}
              value={secureBackupSetupStatusLabel(keyManagement.secure_backup_setup)}
              testId="secure-backup-state"
            />
            <label className="profile-settings-field">
              <span>{t("settings.secureBackupPassphrase")}</span>
              <SecureImeTextField
                ref={secureBackupPassphraseRef}
                autoComplete="new-password"
              />
            </label>
            <div className="profile-settings-actions">
              <button className="trust-action-button primary" type="submit">
                <KeyRound size={14} />
                <span>{t("settings.setupSecureBackup")}</span>
              </button>
            </div>
          </ImeSafeForm>

          <ImeSafeForm
            aria-label={t("settings.changeSecureBackupPassphrase")}
            className="profile-settings-form"
            onSubmit={submitSecureBackupPassphraseChange}
          >
            <KeyManagementStatus
              label={t("settings.changeSecureBackupPassphrase")}
              value={secureBackupPassphraseChangeStatusLabel(keyManagement.passphrase_change)}
              testId="secure-backup-passphrase-change-state"
            />
            <label className="profile-settings-field">
              <span>{t("settings.oldSecureBackupSecret")}</span>
              <SecureImeTextField
                ref={oldSecureBackupSecretRef}
                autoComplete="current-password"
              />
            </label>
            <label className="profile-settings-field">
              <span>{t("settings.newSecureBackupPassphrase")}</span>
              <SecureImeTextField
                ref={newSecureBackupPassphraseRef}
                autoComplete="new-password"
              />
            </label>
            <div className="profile-settings-actions">
              <button className="trust-action-button primary" type="submit">
                <RefreshCcw size={14} />
                <span>{t("settings.updateSecureBackupPassphrase")}</span>
              </button>
            </div>
          </ImeSafeForm>
        </div>
      </section>
      {roomKeyPassphraseRequest ? (
        <div className="dialog-overlay" role="presentation">
          <ImeSafeForm
            className="dialog-box"
            role="dialog"
            aria-modal="true"
            aria-labelledby="room-key-passphrase-title"
            onSubmit={submitRoomKeyPassphrase}
          >
            <h3 className="dialog-title" id="room-key-passphrase-title">
              {t("settings.roomKeyPassphrase")}
            </h3>
            <p className="profile-settings-hint">
              {roomKeyPassphraseRequest.kind === "export"
                ? t("settings.roomKeyPassphrasePromptExport")
                : t("settings.roomKeyPassphrasePromptImport")}
            </p>
            <SecureImeTextField
              className="dialog-input"
              ref={roomKeyPassphraseRef}
              autoComplete="new-password"
              aria-label={t("settings.roomKeyPassphrase")}
            />
            <div className="dialog-actions">
              <button className="dialog-button" type="button" onClick={closeRoomKeyPassphraseDialog}>
                {t("action.cancel")}
              </button>
              <button className="dialog-button is-primary" type="submit">
                {roomKeyPassphraseRequest.kind === "export"
                  ? t("settings.exportRoomKeys")
                  : t("settings.importRoomKeys")}
              </button>
            </div>
          </ImeSafeForm>
        </div>
      ) : null}
    </>
  );
}

type RoomKeyPassphraseRequest =
  | { kind: "export"; path: string }
  | { kind: "import"; path: string };

function KeyManagementStatus({
  label,
  value,
  testId
}: {
  label: string;
  value: string;
  testId: string;
}) {
  return (
    <div className="settings-detail-row">
      <span>{label}</span>
      <small data-testid={testId}>{value}</small>
    </div>
  );
}

function TrustSection({
  trust,
  onBootstrapCrossSigning,
  onEnableKeyBackup,
  onAcceptVerification,
  onConfirmSasVerification,
  onCancelVerification,
  onResetIdentity,
  onCancelIdentityReset,
  onSubmitIdentityResetPassword,
  onSubmitIdentityResetOAuth
}: {
  trust: E2eeTrustState;
  onBootstrapCrossSigning: () => void;
  onEnableKeyBackup: () => void;
  onAcceptVerification: (flowId: number) => void;
  onConfirmSasVerification: (flowId: number) => void;
  onCancelVerification: (flowId: number) => void;
  onResetIdentity: () => void;
  onCancelIdentityReset: (flowId: number) => void;
  onSubmitIdentityResetPassword: (flowId: number, password: string) => void;
  onSubmitIdentityResetOAuth: (flowId: number) => void;
}) {
  const overall = trustOverallStatus(trust);

  return (
    <section className="settings-section trust-section" aria-label={t("trust.encryption")}>
      <div className="settings-section-heading">
        <h3>{t("trust.encryption")}</h3>
        <span className={`trust-status-chip ${overall.tone}`}>{overall.label}</span>
      </div>

      <VerificationDialog
        verification={trust.verification}
        onAccept={onAcceptVerification}
        onCancel={onCancelVerification}
        onConfirm={onConfirmSasVerification}
      />

      <div className="trust-status-list">
        <TrustStatusRow
          icon={<ShieldCheck size={16} />}
          label={t("trust.crossSigning")}
          value={crossSigningStatusLabel(trust.cross_signing)}
          tone={crossSigningTone(trust.cross_signing)}
          action={
            crossSigningActionAvailable(trust.cross_signing) ? (
              <TrustActionButton
                icon={<ShieldCheck size={14} />}
                label={t("trust.setupCrossSigning")}
                onClick={onBootstrapCrossSigning}
              />
            ) : null
          }
        />
        <TrustStatusRow
          icon={<KeyRound size={16} />}
          label={t("trust.keyBackup")}
          value={keyBackupStatusLabel(trust.key_backup)}
          tone={keyBackupTone(trust.key_backup)}
          action={
            keyBackupActionAvailable(trust.key_backup) ? (
              <TrustActionButton
                icon={<KeyRound size={14} />}
                label={t("trust.enableKeyBackup")}
                onClick={onEnableKeyBackup}
              />
            ) : null
          }
        />
        <TrustStatusRow
          icon={<RotateCcw size={16} />}
          label={t("trust.identityReset")}
          value={identityResetStatusLabel(trust.identity_reset)}
          tone={identityResetTone(trust.identity_reset)}
          action={
            trust.identity_reset.kind === "resetting" ? null : (
              <TrustActionButton
                icon={<RotateCcw size={14} />}
                label={t("trust.resetIdentity")}
                onClick={onResetIdentity}
              />
            )
          }
        />
      </div>

      <IdentityResetAuthControls
        state={trust.identity_reset}
        onCancelIdentityReset={onCancelIdentityReset}
        onSubmitIdentityResetOAuth={onSubmitIdentityResetOAuth}
        onSubmitIdentityResetPassword={onSubmitIdentityResetPassword}
      />

      <DeviceTrustList devices={trust.devices} />
    </section>
  );
}

function VerificationDialog({
  verification,
  onAccept,
  onCancel,
  onConfirm
}: {
  verification: VerificationFlowState;
  onAccept: (flowId: number) => void;
  onCancel: (flowId: number) => void;
  onConfirm: (flowId: number) => void;
}) {
  if (verification.kind === "idle") {
    return null;
  }

  const titleId = `trust-verification-${verification.request_id}`;
  const flowId = verification.request_id;
  const statusLabel = verificationStatusLabel(verification);

  return (
    <article
      className={`trust-verification-dialog ${verification.kind}`}
      role="dialog"
      aria-labelledby={titleId}
    >
      <div className="trust-verification-heading">
        <ShieldQuestion size={17} aria-hidden="true" />
        <div>
          <h4 id={titleId}>{t("trust.verification")}</h4>
          <p>{statusLabel}</p>
        </div>
      </div>

      {verification.kind === "sasPresented" || verification.kind === "confirming" ? (
        <ol className="trust-sas-list" aria-label={t("trust.sasEmojiList")}>
          {verification.emojis.map((emoji, index) => (
            <li
              className="trust-sas-item"
              key={`${emoji.symbol}-${index}`}
              aria-label={t("trust.sasEmoji", { index: index + 1 })}
            >
              {emoji.symbol}
            </li>
          ))}
        </ol>
      ) : null}

      {verification.kind === "requested" ? (
        <div className="trust-dialog-actions">
          <TrustActionButton
            icon={<Check size={14} />}
            label={t("trust.acceptVerification")}
            onClick={() => onAccept(flowId)}
          />
          <TrustActionButton
            icon={<X size={14} />}
            label={t("trust.declineVerification")}
            variant="secondary"
            onClick={() => onCancel(flowId)}
          />
        </div>
      ) : null}

      {verification.kind === "sasPresented" ? (
        <div className="trust-dialog-actions">
          <TrustActionButton
            icon={<Check size={14} />}
            label={t("trust.confirmSas")}
            onClick={() => onConfirm(flowId)}
          />
          <TrustActionButton
            icon={<X size={14} />}
            label={t("trust.declineVerification")}
            variant="secondary"
            onClick={() => onCancel(flowId)}
          />
        </div>
      ) : null}

      {verification.kind === "accepted" ||
      verification.kind === "confirming" ||
      verification.kind === "failed" ? (
        <div className="trust-dialog-actions">
          <TrustActionButton
            icon={<X size={14} />}
            label={t("trust.closeVerification")}
            variant="secondary"
            onClick={() => onCancel(flowId)}
          />
        </div>
      ) : null}
    </article>
  );
}

function credentialStoreLabel(platform: DisplayPlatform): string {
  switch (platform) {
    case "macos":
      return t("settings.credentialStoreMacos");
    case "windows":
      return t("settings.credentialStoreWindows");
    case "linux":
      return t("settings.credentialStoreLinux");
  }
}

function localEncryptionStatus(state: LocalEncryptionState): {
  label: string;
  tone: TrustTone;
  icon: ReactNode;
} {
  switch (state.kind) {
    case "healthy":
      return {
        label: t("settings.localEncryptionHealthy"),
        tone: "good",
        icon: <ShieldCheck size={16} />
      };
    case "probing":
      return {
        label: t("settings.localEncryptionChecking"),
        tone: "progress",
        icon: <RefreshCcw size={16} />
      };
    case "unavailable":
      return {
        label: t("settings.localEncryptionUnavailable"),
        tone: "danger",
        icon: <ShieldX size={16} />
      };
    case "lockedOrInaccessible":
      return {
        label: t("settings.localEncryptionLocked"),
        tone: "warning",
        icon: <ShieldAlert size={16} />
      };
    case "missingCredential":
      return {
        label: t("settings.localEncryptionMissing"),
        tone: "danger",
        icon: <ShieldX size={16} />
      };
    case "resetRequired":
      return {
        label: t("settings.localEncryptionResetRequired"),
        tone: "danger",
        icon: <ShieldX size={16} />
      };
    case "resetting":
      return {
        label: t("settings.localEncryptionResetting"),
        tone: "progress",
        icon: <RefreshCcw size={16} />
      };
    case "unknown":
      return {
        label: t("settings.localEncryptionUnknown"),
        tone: "neutral",
        icon: <ShieldQuestion size={16} />
      };
  }
}

function roomKeyExportStatusLabel(status: RoomKeyExportState): string {
  switch (status.kind) {
    case "idle":
      return t("settings.roomKeyExportIdle");
    case "exporting":
      return t("settings.roomKeyExporting");
    case "exported":
      return status.exported_sessions === null
        ? t("settings.roomKeyExportedUnknown")
        : t("settings.roomKeyExportedCount", { count: status.exported_sessions });
    case "failed":
      return t("settings.roomKeyExportFailed", {
        reason: failureKindLabel(status.failureKind)
      });
  }
}

function roomKeyImportStatusLabel(status: RoomKeyImportState): string {
  switch (status.kind) {
    case "idle":
      return t("settings.roomKeyImportIdle");
    case "importing":
      return t("settings.roomKeyImporting");
    case "imported":
      return t("settings.roomKeyImportedCount", {
        imported: status.imported_count,
        total: status.total_count
      });
    case "failed":
      return t("settings.roomKeyImportFailed", {
        reason: failureKindLabel(status.failureKind)
      });
  }
}

function secureBackupSetupStatusLabel(status: SecureBackupSetupState): string {
  switch (status.kind) {
    case "idle":
      return t("settings.secureBackupIdle");
    case "settingUp":
      return t("settings.secureBackupSettingUp");
    case "recoveryKeyReady":
      return recoveryKeyDeliveryLabel(status.delivery);
    case "enabled":
      return t("settings.secureBackupEnabled");
    case "failed":
      return t("settings.secureBackupFailed", {
        reason: failureKindLabel(status.failureKind)
      });
  }
}

function secureBackupPassphraseChangeStatusLabel(
  status: SecureBackupPassphraseChangeState
): string {
  switch (status.kind) {
    case "idle":
      return t("settings.passphraseChangeIdle");
    case "changing":
      return t("settings.passphraseChangeChanging");
    case "changed":
      return status.delivery.kind === "written"
        ? t("settings.passphraseChangeRecoveryKeySaved")
        : t("settings.passphraseChangeChanged");
    case "failed":
      return t("settings.passphraseChangeFailed", {
        reason: failureKindLabel(status.failureKind)
      });
  }
}

function recoveryKeyDeliveryLabel(delivery: RecoveryKeyDeliveryState): string {
  switch (delivery.kind) {
    case "written":
      return t("settings.recoveryKeySaved");
    case "notWritten":
      return t("settings.recoveryKeyReady");
  }
}

function IdentityResetAuthControls({
  state,
  onCancelIdentityReset,
  onSubmitIdentityResetPassword,
  onSubmitIdentityResetOAuth
}: {
  state: IdentityResetState;
  onCancelIdentityReset: (flowId: number) => void;
  onSubmitIdentityResetPassword: (flowId: number, password: string) => void;
  onSubmitIdentityResetOAuth: (flowId: number) => void;
}) {
  const passwordInput = useRef<HTMLInputElement>(null);
  const [passwordFilled, setPasswordFilled] = useState(false);

  if (state.kind !== "awaitingAuth") {
    return null;
  }

  const flowId = state.request_id;

  if (state.auth_type === "oauth") {
    return (
      <div className="trust-auth-row">
        <TrustActionButton
          icon={<X size={14} />}
          label={t("trust.cancelIdentityReset")}
          onClick={() => onCancelIdentityReset(flowId)}
        />
        <TrustActionButton
          icon={<Check size={14} />}
          label={t("trust.continueIdentityReset")}
          onClick={() => onSubmitIdentityResetOAuth(flowId)}
        />
      </div>
    );
  }

  if (state.auth_type !== "uiaa") {
    return (
      <div className="trust-auth-row" role="status">
        <ShieldAlert size={15} aria-hidden="true" />
        <span>{t("trust.identityResetAuthUnknown")}</span>
        <TrustActionButton
          icon={<X size={14} />}
          label={t("trust.cancelIdentityReset")}
          onClick={() => onCancelIdentityReset(flowId)}
        />
      </div>
    );
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const password = passwordInput.current?.value ?? "";
    if (!password) {
      return;
    }
    onSubmitIdentityResetPassword(flowId, password);
    if (passwordInput.current) {
      passwordInput.current.value = "";
    }
    setPasswordFilled(false);
  }

  return (
    <ImeSafeForm className="trust-auth-row" onSubmit={submit}>
      <label className="trust-password-field">
        <span>{t("trust.identityResetPassword")}</span>
        <SecureImeTextField
          autoComplete="current-password"
          ref={passwordInput}
          onInput={(event) => setPasswordFilled(event.currentTarget.value.length > 0)}
        />
      </label>
      <button className="trust-action-button primary" type="submit" disabled={!passwordFilled}>
        <Check size={14} />
        <span>{t("trust.continueIdentityReset")}</span>
      </button>
      <button
        className="trust-action-button"
        type="button"
        onClick={() => onCancelIdentityReset(flowId)}
      >
        <X size={14} />
        <span>{t("trust.cancelIdentityReset")}</span>
      </button>
    </ImeSafeForm>
  );
}

function DeviceTrustList({ devices }: { devices: E2eeTrustState["devices"] }) {
  return (
    <section className="trust-devices" aria-label={t("trust.devices")}>
      <div className="trust-devices-heading">
        <h4>
          <span>{t("trust.devices")}</span>
          <TrustHelpButton
            title={t("help.userTrust.deviceStateTitle")}
            body={t("help.userTrust.deviceStateBody")}
          />
        </h4>
        <span>{t("trust.deviceCount", { count: devices.length })}</span>
      </div>
      <div className="trust-device-list">
        {devices.length > 0 ? (
          devices.map((device, index) => (
            <div className="trust-device-row" key={`${device.user_id}|${device.device_id}`}>
              <span className={`trust-device-icon ${device.trust_level}`} aria-hidden="true">
                {deviceTrustIcon(device.trust_level)}
              </span>
              <span className="trust-device-copy">
                <span>{t("trust.deviceOrdinal", { index: index + 1 })}</span>
                <small>{deviceTrustLevelLabel(device.trust_level)}</small>
              </span>
            </div>
          ))
        ) : (
          <div className="trust-device-row">
            <span className="trust-device-icon unknown" aria-hidden="true">
              <ShieldQuestion size={15} />
            </span>
            <span className="trust-device-copy">
              <span>{t("trust.noDevices")}</span>
              <small>{t("trust.statusUnknown")}</small>
            </span>
          </div>
        )}
      </div>
    </section>
  );
}

function trustOverallStatus(trust: E2eeTrustState): { label: string; tone: TrustTone } {
  if (
    trust.verification.kind === "failed" ||
    trust.cross_signing.kind === "failed" ||
    trust.key_backup.kind === "failed" ||
    trust.identity_reset.kind === "failed"
  ) {
    return { label: t("trust.statusFailed"), tone: "danger" };
  }

  if (
    trust.verification.kind === "requested" ||
    trust.verification.kind === "accepted" ||
    trust.verification.kind === "sasPresented" ||
    trust.verification.kind === "confirming" ||
    trust.cross_signing.kind === "bootstrapping" ||
    trust.key_backup.kind === "enabling" ||
    trust.key_backup.kind === "restoring" ||
    trust.identity_reset.kind === "resetting" ||
    trust.identity_reset.kind === "awaitingAuth"
  ) {
    return { label: t("trust.statusInProgress"), tone: "progress" };
  }

  if (
    trust.cross_signing.kind === "trusted" &&
    trust.key_backup.kind === "enabled" &&
    trust.devices.length > 0 &&
    trust.devices.every((device) => device.trust_level === "verified")
  ) {
    return { label: t("trust.statusTrusted"), tone: "good" };
  }

  if (
    trust.cross_signing.kind === "unknown" &&
    trust.key_backup.kind === "unknown" &&
    trust.devices.length === 0
  ) {
    return { label: t("trust.statusUnknown"), tone: "neutral" };
  }

  return { label: t("trust.statusNeedsAttention"), tone: "warning" };
}

function crossSigningStatusLabel(status: CrossSigningStatus): string {
  switch (status.kind) {
    case "unknown":
      return t("trust.statusUnknown");
    case "missing":
      return t("trust.statusMissing");
    case "bootstrapping":
      return t("trust.statusBootstrapping");
    case "trusted":
      return t("trust.statusTrusted");
    case "notTrusted":
      return t("trust.statusNotTrusted");
    case "failed":
      return t("trust.statusFailedReason", {
        reason: failureKindLabel(status.failureKind)
      });
  }
}

function keyBackupStatusLabel(status: KeyBackupStatus): string {
  switch (status.kind) {
    case "unknown":
      return t("trust.statusUnknown");
    case "disabled":
      return t("trust.statusDisabled");
    case "enabling":
      return t("trust.statusEnabling");
    case "enabled":
      return t("trust.statusEnabled");
    case "restoring":
      return status.total_rooms === null
        ? t("trust.statusRestoringBackupOpen", {
            restored: status.restored_rooms
          })
        : t("trust.statusRestoringBackup", {
            restored: status.restored_rooms,
            total: status.total_rooms
          });
    case "failed":
      return t("trust.statusFailedReason", {
        reason: failureKindLabel(status.failureKind)
      });
  }
}

function identityResetStatusLabel(status: IdentityResetState): string {
  switch (status.kind) {
    case "idle":
      return t("trust.statusIdle");
    case "resetting":
      return t("trust.statusResetting");
    case "awaitingAuth":
      return t("trust.statusAwaitingAuth");
    case "failed":
      return t("trust.statusFailedReason", {
        reason: failureKindLabel(status.failureKind)
      });
  }
}

function verificationStatusLabel(status: VerificationFlowState): string {
  switch (status.kind) {
    case "idle":
      return t("trust.statusIdle");
    case "requested":
      return t("trust.statusVerificationRequested");
    case "accepted":
      return t("trust.statusVerificationAccepted");
    case "sasPresented":
      return t("trust.statusSasPresented");
    case "confirming":
      return t("trust.statusConfirming");
    case "done":
      return t("trust.statusVerified");
    case "failed":
      return t("trust.statusFailedReason", {
        reason: failureKindLabel(status.failureKind)
      });
  }
}

function deviceTrustLevelLabel(level: DeviceTrustLevel): string {
  switch (level) {
    case "unknown":
      return t("trust.deviceUnknown");
    case "unverified":
      return t("trust.deviceNotCrossSigned");
    case "verified":
      return t("trust.deviceCrossSigned");
    case "blocked":
      return t("trust.deviceBlocked");
  }
}

function deviceTrustIcon(level: DeviceTrustLevel): ReactNode {
  switch (level) {
    case "verified":
      return <ShieldCheck size={15} />;
    case "blocked":
      return <ShieldX size={15} />;
    case "unknown":
      return <ShieldQuestion size={15} />;
    case "unverified":
      return <ShieldAlert size={15} />;
  }
}

function crossSigningTone(status: CrossSigningStatus): TrustTone {
  switch (status.kind) {
    case "trusted":
      return "good";
    case "bootstrapping":
      return "progress";
    case "failed":
      return "danger";
    case "unknown":
      return "neutral";
    case "missing":
    case "notTrusted":
      return "warning";
  }
}

function keyBackupTone(status: KeyBackupStatus): TrustTone {
  switch (status.kind) {
    case "enabled":
      return "good";
    case "enabling":
    case "restoring":
      return "progress";
    case "failed":
      return "danger";
    case "unknown":
      return "neutral";
    case "disabled":
      return "warning";
  }
}

function identityResetTone(status: IdentityResetState): TrustTone {
  switch (status.kind) {
    case "idle":
      return "neutral";
    case "resetting":
    case "awaitingAuth":
      return "progress";
    case "failed":
      return "danger";
  }
}

function crossSigningActionAvailable(status: CrossSigningStatus): boolean {
  return (
    status.kind === "unknown" ||
    status.kind === "missing" ||
    status.kind === "notTrusted" ||
    status.kind === "failed"
  );
}

function keyBackupActionAvailable(status: KeyBackupStatus): boolean {
  return status.kind === "unknown" || status.kind === "disabled" || status.kind === "failed";
}

function ThemeButton({
  label,
  selected,
  value,
  onSelect
}: {
  label: string;
  selected: boolean;
  value: ThemePreference;
  onSelect: (patch: SettingsPatch) => void;
}) {
  return (
    <button
      className={`segmented-control-option ${selected ? "is-selected" : ""}`}
      type="button"
      aria-pressed={selected}
      onClick={() => {
        if (!selected) {
          onSelect({ appearance: { theme: value } });
        }
      }}
    >
      {label}
    </button>
  );
}

function DensityButton({
  label,
  selected,
  value,
  onSelect
}: {
  label: string;
  selected: boolean;
  value: DisplayDensity;
  onSelect: (density: DisplayDensity) => void;
}) {
  return (
    <button
      className={`segmented-control-option ${selected ? "is-selected" : ""}`}
      type="button"
      aria-pressed={selected}
      onClick={() => {
        if (!selected) {
          onSelect(value);
        }
      }}
    >
      {label}
    </button>
  );
}

function FontButton({
  label,
  selected,
  value,
  currentEmoji,
  onSelect
}: {
  label: string;
  selected: boolean;
  value: FontPreference;
  currentEmoji: EmojiPreference;
  onSelect: (patch: SettingsPatch) => void;
}) {
  return (
    <button
      className={`segmented-control-option ${selected ? "is-selected" : ""}`}
      type="button"
      aria-pressed={selected}
      onClick={() => {
        if (!selected) {
          onSelect({ typography: { font: value, emoji: currentEmoji } });
        }
      }}
    >
      {label}
    </button>
  );
}

function EmojiButton({
  label,
  selected,
  value,
  currentFont,
  onSelect
}: {
  label: string;
  selected: boolean;
  value: EmojiPreference;
  currentFont: FontPreference;
  onSelect: (patch: SettingsPatch) => void;
}) {
  return (
    <button
      className={`segmented-control-option ${selected ? "is-selected" : ""}`}
      type="button"
      aria-pressed={selected}
      onClick={() => {
        if (!selected) {
          onSelect({ typography: { font: currentFont, emoji: value } });
        }
      }}
    >
      {label}
    </button>
  );
}

function NotificationSettingToggle({
  label,
  settingKey,
  current,
  onSelect,
  icon
}: {
  label: string;
  settingKey: keyof NotificationSettings;
  current: NotificationSettings;
  onSelect: (patch: SettingsPatch) => void;
  icon: ReactNode;
}) {
  const checked = current[settingKey];
  return (
    <button
      className="settings-toggle-row"
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => {
        onSelect({
          notifications: {
            ...current,
            [settingKey]: !checked
          }
        });
      }}
    >
      <span className="settings-toggle-copy">
        <span className="settings-toggle-label">
          {icon}
          <span>{label}</span>
        </span>
      </span>
      <span className="settings-switch-track" aria-hidden="true">
        <span className="settings-switch-thumb" />
      </span>
    </button>
  );
}

function TimelineToggle({
  label,
  description,
  settingKey,
  current,
  onSelect
}: {
  label: string;
  description?: string;
  settingKey: "auto_load_older_messages";
  current: TimelineSettings;
  onSelect: (patch: SettingsPatch) => void;
}) {
  const checked = current[settingKey];
  return (
    <button
      className="settings-toggle-row"
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => {
        onSelect({
          timeline: {
            ...current,
            [settingKey]: !checked
          }
        });
      }}
    >
      <span className="settings-toggle-copy">
        <span className="settings-toggle-label">
          <History size={15} aria-hidden="true" />
          <span>{label}</span>
        </span>
        {description ? (
          <span className="settings-toggle-description">{description}</span>
        ) : null}
      </span>
      <span className="settings-switch-track" aria-hidden="true">
        <span className="settings-switch-thumb" />
      </span>
    </button>
  );
}

function TimelineThreadRootOrderToggle({
  label,
  description,
  current,
  onSelect
}: {
  label: string;
  description: string;
  current: TimelineSettings;
  onSelect: (patch: SettingsPatch) => void;
}) {
  const checked = current.thread_root_order.kind === "latestReply";
  return (
    <button
      className="settings-toggle-row"
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => {
        onSelect({
          timeline: {
            ...current,
            thread_root_order: { kind: checked ? "rootEvent" : "latestReply" }
          }
        });
      }}
    >
      <span className="settings-toggle-copy">
        <span className="settings-toggle-label">
          <History size={15} aria-hidden="true" />
          <span>{label}</span>
        </span>
        <span className="settings-toggle-description">{description}</span>
      </span>
      <span className="settings-switch-track" aria-hidden="true">
        <span className="settings-switch-thumb" />
      </span>
    </button>
  );
}

function DisplayToggle({
  label,
  description,
  settingKey,
  icon,
  current,
  onSelect
}: {
  label: string;
  description?: string;
  settingKey: keyof DisplaySettings;
  icon: "code" | "hideRedacted" | "link";
  current: DisplaySettings;
  onSelect: (patch: SettingsPatch) => void;
}) {
  const checked = current[settingKey];
  const Icon = icon === "code" ? Code2 : icon === "hideRedacted" ? EyeOff : Link;
  return (
    <button
      className="settings-toggle-row"
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => {
        onSelect({
          display: {
            ...current,
            [settingKey]: !checked
          }
        });
      }}
    >
      <span className="settings-toggle-copy">
        <span className="settings-toggle-label">
          <Icon size={15} aria-hidden="true" />
          <span>{label}</span>
        </span>
        {description ? (
          <span className="settings-toggle-description">{description}</span>
        ) : null}
      </span>
      <span className="settings-switch-track" aria-hidden="true">
        <span className="settings-switch-thumb" />
      </span>
    </button>
  );
}

function sessionMatches(left: SavedSessionInfo | null, right: SavedSessionInfo): boolean {
  return (
    left?.homeserver === right.homeserver &&
    left.user_id === right.user_id &&
    left.device_id === right.device_id
  );
}

function sessionKey(session: SavedSessionInfo): string {
  return `${session.homeserver}|${session.user_id}|${session.device_id}`;
}

function avatarSourceUrl(avatar: ProfileState["own"]["avatar"]): string | null {
  if (avatar?.thumbnail.kind !== "ready") {
    return null;
  }
  return mediaSourceUrl(avatar.thumbnail.source_url);
}

function accountInitial(userId: string): string {
  return userId.replace(/^@/, "").charAt(0).toUpperCase() || "?";
}
