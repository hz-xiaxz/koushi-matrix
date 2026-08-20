import { join } from "node:path";
import { safeTimestamp } from "../evidence.mjs";
import { cleanupLocalGuiScenario,recordLocalGuiEvidence,startLocalGuiScenario,waitForAuthScreen,waitForLocalLoginReady,writeLocalLoginPipe } from "../local-session.mjs";
import { timeoutMs } from "../options.mjs";
import { clickKeyManagementFormButton,ensureUserSettingsKeyManagementOpen,setKeyManagementFormInput,waitForDocumentText,waitForDocumentTheme,waitForElementAttribute,waitForFileExists,waitForKeyManagementStatus,waitForSecureBackupSetupEvidence } from "../webdriver.mjs";

export async function runLocalSettingsScenario() {
  const session = await startLocalGuiScenario();
  try {
    await waitForAuthScreen(session.browser, timeoutMs);
    await writeLocalLoginPipe(session.qaLoginPipePath, session.credentials);
    await waitForLocalLoginReady(session, timeoutMs);

    const keyboardSettings = await session.browser.$('button[aria-label="Keyboard settings"]');
    await keyboardSettings.waitForDisplayed({ timeout: timeoutMs });
    await keyboardSettings.click();
    const modEnterButtonSelector =
      "//button[normalize-space()='Ctrl+Enter sends' or normalize-space()='Cmd+Enter sends']";
    const modEnterButton = await session.browser.$(modEnterButtonSelector);
    await modEnterButton.waitForDisplayed({ timeout: timeoutMs });
    await modEnterButton.click();
    await waitForElementAttribute(
      session.browser,
      modEnterButtonSelector,
      "aria-pressed",
      "true",
      timeoutMs,
      "composer shortcut setting"
    );

    const userSettings = await session.browser.$('button[aria-label="User settings"]');
    await userSettings.waitForDisplayed({ timeout: timeoutMs });
    await userSettings.click();
    const darkThemeButton = await session.browser.$("//button[normalize-space()='Dark']");
    await darkThemeButton.waitForDisplayed({ timeout: timeoutMs });
    await darkThemeButton.click();
    await waitForElementAttribute(
      session.browser,
      "//button[normalize-space()='Dark']",
      "aria-pressed",
      "true",
      timeoutMs,
      "dark theme setting"
    );
    await waitForDocumentTheme(session.browser, "dark", timeoutMs);
    await waitForDocumentText(
      session.browser,
      ["Encryption", "Cross-signing", "Key backup", "Identity reset", "Devices"],
      timeoutMs,
      "E2EE trust settings section"
    );

    await recordLocalGuiEvidence(session);
    console.log("gui_local_settings=ok");
    console.log("gui_local_trust_settings=ok");
  } finally {
    await cleanupLocalGuiScenario(session);
  }
}

export async function runLocalE2eeKeyManagementScenario() {
  const session = await startLocalGuiScenario();
  try {
    await waitForAuthScreen(session.browser, timeoutMs);
    await writeLocalLoginPipe(session.qaLoginPipePath, session.credentials);
    await waitForLocalLoginReady(session, timeoutMs);

    await ensureUserSettingsKeyManagementOpen(session.browser, timeoutMs);

    const keyFilePath = join(session.runDir, "room-keys.txt");
    const recoveryKeyPath = join(session.runDir, "secure-backup-recovery.txt");
    const keyFilePassphrase = `koushi-key-transfer-${safeTimestamp()}`;
    const secureBackupPassphrase = `koushi-desktop-secure-backup-${safeTimestamp()}`;

    await setKeyManagementFormInput(
      session.browser,
      "Room key export",
      "Key export destination",
      keyFilePath
    );
    await setKeyManagementFormInput(
      session.browser,
      "Room key export",
      "Room key passphrase",
      keyFilePassphrase
    );
    await clickKeyManagementFormButton(
      session.browser,
      "Room key export",
      "Export room keys",
      timeoutMs
    );
    await waitForKeyManagementStatus(
      session.browser,
      "room-key-export-state",
      ["Exported", "sessions exported"],
      timeoutMs,
      "local GUI room-key export"
    );
    await waitForFileExists(keyFilePath, timeoutMs, "local GUI room-key export artifact");
    console.log("gui_room_key_export=ok");

    await setKeyManagementFormInput(
      session.browser,
      "Room key import",
      "Key import source",
      keyFilePath
    );
    await setKeyManagementFormInput(
      session.browser,
      "Room key import",
      "Room key passphrase",
      keyFilePassphrase
    );
    await clickKeyManagementFormButton(
      session.browser,
      "Room key import",
      "Import room keys",
      timeoutMs
    );
    await waitForKeyManagementStatus(
      session.browser,
      "room-key-import-state",
      ["imported"],
      timeoutMs,
      "local GUI room-key import"
    );
    console.log("gui_room_key_import=ok");

    await setKeyManagementFormInput(
      session.browser,
      "Secure backup",
      "Secure backup passphrase",
      secureBackupPassphrase
    );
    await setKeyManagementFormInput(
      session.browser,
      "Secure backup",
      "Recovery key destination",
      recoveryKeyPath
    );
    await waitForKeyManagementStatus(
      session.browser,
      "secure-backup-state",
      ["Not set up"],
      timeoutMs,
      "local GUI secure-backup initial status"
    );
    await clickKeyManagementFormButton(
      session.browser,
      "Secure backup",
      "Set up secure backup",
      timeoutMs
    );
    await waitForFileExists(recoveryKeyPath, timeoutMs, "local GUI secure-backup artifact");
    await waitForSecureBackupSetupEvidence(session.browser, timeoutMs);
    console.log("gui_secure_backup_setup=ok");
  } finally {
    await cleanupLocalGuiScenario(session);
  }
}
