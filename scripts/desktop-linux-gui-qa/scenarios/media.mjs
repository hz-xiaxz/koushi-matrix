import { readFileSync,writeFileSync } from "node:fs";
import { join } from "node:path";
import { sendRoomEmoteMessage,sendRoomFormattedMessage,sendRoomNoticeMessage } from "../../lib/local-homeserver-qa.mjs";
import { safeTimestamp } from "../evidence.mjs";
import { cleanupLocalGuiScenario,recordLocalGuiEvidence,startLocalGuiScenario,waitForAuthScreen,waitForLocalLoginReady,writeLocalLoginPipe } from "../local-session.mjs";
import { timeoutMs } from "../options.mjs";
import { clickReadyComposerSendButton,clickVisibleButtonByAriaLabel,elementCount,ensureReadyImageMedia,readyImageMediaXpath,readyImageOpenButtonXpath,selectRoomByName,setSyntheticFileInput,setTextInputValueByLabel,waitForActiveRoomName,waitForCompressedImageMedia,waitForDocumentText,waitForElementAttribute,waitForElementCount,waitForElementCountGreaterThan,waitForInputValue,waitForQaTitle,waitForReadyImageHoverActions,waitForStagedUpload,waitForStagedUploadsCleared,waitForTimelineViewMounted,writePngFixture } from "../webdriver.mjs";

export async function runLocalMediaScenario() {
  const session = await startLocalGuiScenario();
  try {
    await waitForAuthScreen(session.browser, timeoutMs);
    await writeLocalLoginPipe(session.qaLoginPipePath, session.credentials);
    await waitForLocalLoginReady(session.browser, timeoutMs);
    await selectRoomByName(session.browser, "QA Seed Room", timeoutMs);
    await waitForActiveRoomName(session.browser, "QA Seed Room", timeoutMs);
    await waitForQaTitle(
      session.browser,
      (status) => status.timeline_room === true && status.timeline_subscribed === true,
      timeoutMs,
      "local GUI media timeline room"
    );
    await waitForTimelineViewMounted(session.browser, timeoutMs);

    const baselineMediaRows = await elementCount(session.browser, ".message-media");
    const filename = `qa-media-${safeTimestamp()}.txt`;
    const caption = `QA media caption ${safeTimestamp()}`;
    const fixturePath = join(session.runDir, filename);
    writeFileSync(fixturePath, "Koushi Linux GUI media fixture\n", "utf8");
    const composer = await session.browser.$('textarea[aria-label="Message composer"]');
    await composer.waitForDisplayed({ timeout: timeoutMs });
    await composer.click();
    const fileInputSelector = 'input[type="file"][aria-label="Attach file input"]';
    await setSyntheticFileInput(
      session.browser,
      fileInputSelector,
      fixturePath,
      filename,
      "text/plain",
      "Koushi Linux GUI media fixture"
    );
    await waitForDocumentText(
      session.browser,
      [filename],
      timeoutMs,
      "local GUI staged media preview"
    );
    const captionInput = await session.browser.$(`input[aria-label="Caption for ${filename}"]`);
    await captionInput.waitForDisplayed({ timeout: timeoutMs });
    await setTextInputValueByLabel(session.browser, caption, `Caption for ${filename}`);
    await waitForInputValue(
      session.browser,
      `Caption for ${filename}`,
      caption,
      timeoutMs,
      "local GUI media staging caption"
    );
    const stagedMediaRows = await elementCount(session.browser, ".message-media");
    if (stagedMediaRows !== baselineMediaRows) {
      throw new Error(
        `local GUI media staged attachment sent before Send: baseline=${baselineMediaRows} observed=${stagedMediaRows}`
      );
    }
    console.log("gui_local_media_stage=ok");
    await clickReadyComposerSendButton(
      session.browser,
      timeoutMs,
      "local GUI media send"
    );
    await waitForStagedUploadsCleared(
      session.browser,
      timeoutMs,
      "local GUI media staging clear"
    );
    await waitForElementCountGreaterThan(
      session.browser,
      ".message-media",
      baselineMediaRows,
      timeoutMs,
      "local GUI media render"
    );
    await waitForDocumentText(
      session.browser,
      [filename, caption],
      timeoutMs,
      "local GUI media caption render"
    );

    const downloadButton = await session.browser.$(`button[aria-label="Download ${filename}"]`);
    await downloadButton.waitForDisplayed({ timeout: timeoutMs });
    await downloadButton.click();
    await waitForQaTitle(
      session.browser,
      (status) => status.errors === 0,
      timeoutMs,
      "local GUI media download"
    );

    const galleryButton = await session.browser.$('button[aria-label="Open media gallery"]');
    await galleryButton.waitForDisplayed({ timeout: timeoutMs });
    await galleryButton.click();
    const galleryRegion = await session.browser.$('[role="region"][aria-label="Room media gallery"]');
    await galleryRegion.waitForDisplayed({ timeout: timeoutMs });
    await clickVisibleButtonByAriaLabel(
      session.browser,
      `Open ${filename}`,
      timeoutMs,
      "local GUI media gallery item",
      '[role="region"][aria-label="Room media gallery"]'
    );
    const mediaViewer = await session.browser.$('[role="dialog"][aria-label="Media viewer"]');
    await mediaViewer.waitForDisplayed({ timeout: timeoutMs });
    await waitForDocumentText(
      session.browser,
      [filename],
      timeoutMs,
      "local GUI media viewer"
    );
    const closeViewer = await session.browser.$('button[aria-label="Close media viewer"]');
    await closeViewer.waitForDisplayed({ timeout: timeoutMs });
    await closeViewer.click();
    await waitForElementCount(
      session.browser,
      '[role="dialog"][aria-label="Media viewer"]',
      0,
      timeoutMs,
      "local GUI media gallery viewer close"
    );
    await galleryButton.click();
    await waitForElementCount(
      session.browser,
      '[role="region"][aria-label="Room media gallery"]',
      0,
      timeoutMs,
      "local GUI media gallery close"
    );

    const imageBaselineMediaRows = await elementCount(session.browser, ".message-media");
    const imageFilename = `qa-inline-image-${safeTimestamp()}.png`;
    const imageFixturePath = join(session.runDir, imageFilename);
    writePngFixture(imageFixturePath, 320, 180);
    await setSyntheticFileInput(
      session.browser,
      fileInputSelector,
      imageFixturePath,
      imageFilename,
      "image/png",
      { base64: readFileSync(imageFixturePath).toString("base64") }
    );
    await waitForStagedUpload(
      session.browser,
      imageFilename,
      timeoutMs,
      "local GUI inline image staged preview"
    );
    const stagedImageMediaRows = await elementCount(session.browser, ".message-media");
    if (stagedImageMediaRows !== imageBaselineMediaRows) {
      throw new Error(
        `local GUI inline image sent before Send: baseline=${imageBaselineMediaRows} observed=${stagedImageMediaRows}`
      );
    }
    await clickReadyComposerSendButton(
      session.browser,
      timeoutMs,
      "local GUI inline image send"
    );
    await waitForStagedUploadsCleared(
      session.browser,
      timeoutMs,
      "local GUI inline image staging clear"
    );
    await waitForElementCountGreaterThan(
      session.browser,
      ".message-media",
      imageBaselineMediaRows,
      timeoutMs,
      "local GUI inline image render"
    );
    await ensureReadyImageMedia(session.browser, imageFilename, timeoutMs);
    const readyImage = await session.browser.$(readyImageMediaXpath(imageFilename));
    await readyImage.waitForDisplayed({ timeout: timeoutMs });
    await readyImage.moveTo();
    await waitForReadyImageHoverActions(session.browser, imageFilename, timeoutMs);
    await clickVisibleButtonByAriaLabel(
      session.browser,
      `Show media details for ${imageFilename}`,
      timeoutMs,
      "local GUI inline image details"
    );
    const detailsDialog = await session.browser.$('[role="dialog"][aria-label="Media details"]');
    await detailsDialog.waitForDisplayed({ timeout: timeoutMs });
    await waitForDocumentText(
      session.browser,
      [imageFilename, "image/png", "320x180"],
      timeoutMs,
      "local GUI inline image details"
    );
    await session.browser.keys("Escape");
    await waitForElementCount(
      session.browser,
      '[role="dialog"][aria-label="Media details"]',
      0,
      timeoutMs,
      "local GUI inline image details close"
    );
    const imageOpenButton = await session.browser.$(readyImageOpenButtonXpath(imageFilename));
    await imageOpenButton.waitForDisplayed({ timeout: timeoutMs });
    await imageOpenButton.click();
    const inlineImageViewer = await session.browser.$('[role="dialog"][aria-label="Media viewer"]');
    await inlineImageViewer.waitForDisplayed({ timeout: timeoutMs });
    const viewerFocusState = await session.browser.execute(
      () => document.activeElement?.getAttribute("aria-label") ?? ""
    );
    if (viewerFocusState !== "Close media viewer") {
      throw new Error(`local GUI inline image viewer did not focus close control: ${viewerFocusState}`);
    }
    await waitForDocumentText(
      session.browser,
      [imageFilename, "image/png", "320x180"],
      timeoutMs,
      "local GUI inline image viewer"
    );
    await session.browser.keys("Escape");
    await waitForElementCount(
      session.browser,
      '[role="dialog"][aria-label="Media viewer"]',
      0,
      timeoutMs,
      "local GUI inline image viewer close"
    );

    await recordLocalGuiEvidence(session);
    console.log("gui_local_media=ok");
    console.log("gui_local_media_caption=ok");
    console.log("gui_local_media_viewer=ok");
    console.log("gui_local_media_inline_image=ok");
  } finally {
    await cleanupLocalGuiScenario(session);
  }
}

export async function runLocalImageCompressionScenario() {
  const session = await startLocalGuiScenario();
  try {
    await waitForAuthScreen(session.browser, timeoutMs);
    await writeLocalLoginPipe(session.qaLoginPipePath, session.credentials);
    await waitForLocalLoginReady(session.browser, timeoutMs);
    await waitForQaTitle(
      session.browser,
      (status) => status.timeline_room === true,
      timeoutMs,
      "local GUI image compression timeline room"
    );

    // #305: image output is chosen per attachment in the staging dialog, so this
    // lane drives those controls instead of a retired settings preference.
    await selectRoomByName(session.browser, "QA Seed Room", timeoutMs);
    await waitForActiveRoomName(session.browser, "QA Seed Room", timeoutMs);

    const baselineMediaRows = await elementCount(session.browser, ".message-media");
    const pngFilename = `qa-image-compress-${safeTimestamp()}.png`;
    const jpgFilename = pngFilename.replace(/\.png$/, ".jpg");
    const fixturePath = join(session.runDir, pngFilename);
    writePngFixture(fixturePath, 3000, 10);
    const fileInputSelector = 'input[type="file"][aria-label="Attach file input"]';
    await setSyntheticFileInput(
      session.browser,
      fileInputSelector,
      fixturePath,
      pngFilename,
      "image/png",
      { base64: readFileSync(fixturePath).toString("base64") }
    );
    await waitForDocumentText(
      session.browser,
      [pngFilename],
      timeoutMs,
      "local GUI image compression staged preview"
    );
    const stagedMediaRows = await elementCount(session.browser, ".message-media");
    if (stagedMediaRows !== baselineMediaRows) {
      throw new Error(
        `local GUI image compression sent before Send: baseline=${baselineMediaRows} observed=${stagedMediaRows}`
      );
    }

    // Choose 1/2 and JPEG in the staging dialog, then prove the row that lands
    // is exactly the selected output rather than the source image.
    const halfOption = await session.browser.$(
      '//div[@role="radiogroup" and @aria-label="Resize"]//button[normalize-space()="1/2"]'
    );
    await halfOption.waitForDisplayed({ timeout: timeoutMs });
    await halfOption.click();
    const jpegOption = await session.browser.$(
      '//div[@role="radiogroup" and @aria-label="Format"]//button[normalize-space()="JPEG"]'
    );
    await jpegOption.waitForDisplayed({ timeout: timeoutMs });
    await jpegOption.click();
    await waitForElementAttribute(
      session.browser,
      '//div[@role="status" and @aria-label="Upload result"]',
      "data-upload-output-state",
      "ready",
      timeoutMs,
      "staged upload output recompression"
    );

    const sendButton = await session.browser.$('button[aria-label="Send"]');
    await sendButton.waitForDisplayed({ timeout: timeoutMs });
    await sendButton.click();
    await waitForElementCountGreaterThan(
      session.browser,
      ".message-media",
      baselineMediaRows,
      timeoutMs,
      "local GUI compressed image render"
    );
    await waitForCompressedImageMedia(
      session.browser,
      {
        filename: jpgFilename,
        mimetype: "image/jpeg",
        dimensions: "1500x5"
      },
      timeoutMs
    );

    await recordLocalGuiEvidence(session);
    console.log("gui_local_image_compress=ok");
  } finally {
    await cleanupLocalGuiScenario(session);
  }
}

export async function runLocalMessageTypesScenario() {
  const session = await startLocalGuiScenario();
  try {
    await waitForAuthScreen(session.browser, timeoutMs);
    await writeLocalLoginPipe(session.qaLoginPipePath, session.credentials);
    await waitForLocalLoginReady(session.browser, timeoutMs);
    await selectRoomByName(session.browser, "QA Seed Room", timeoutMs);
    await waitForActiveRoomName(session.browser, "QA Seed Room", timeoutMs);
    await waitForTimelineViewMounted(session.browser, timeoutMs);

    await waitForTimelineViewMounted(session.browser, timeoutMs);
    const baselineEmotes = await elementCount(session.browser, '.message[data-message-kind="emote"]');
    const emoteBody = `waves ${safeTimestamp()}`;
    await sendRoomEmoteMessage(
      session.credentials.homeserver,
      session.helperAccessToken,
      session.seedRoomId,
      emoteBody,
      `qa-emote-${safeTimestamp()}`
    );
    await waitForElementCountGreaterThan(
      session.browser,
      '.message[data-message-kind="emote"]',
      baselineEmotes,
      timeoutMs,
      "local GUI emote render"
    );
    await waitForDocumentText(session.browser, [emoteBody], timeoutMs, "local GUI emote text");
    console.log("gui_local_emote=ok");

    const noticeBody = `QA notice ${safeTimestamp()}`;
    const baselineNotices = await elementCount(
      session.browser,
      '.message[data-message-kind="notice"]'
    );
    await sendRoomNoticeMessage(
      session.credentials.homeserver,
      session.helperAccessToken,
      session.seedRoomId,
      noticeBody,
      `qa-notice-${safeTimestamp()}`
    );
    await waitForElementCountGreaterThan(
      session.browser,
      '.message[data-message-kind="notice"]',
      baselineNotices,
      timeoutMs,
      "local GUI notice render"
    );
    await waitForDocumentText(session.browser, [noticeBody], timeoutMs, "local GUI notice text");
    console.log("gui_local_notice=ok");

    const spoilerSecret = `secret-${safeTimestamp()}`;
    const spoilerBody = `QA spoiler keep ${spoilerSecret} hidden`;
    const baselineSpoilers = await elementCount(session.browser, ".message-spoiler");
    await sendRoomFormattedMessage(
      session.credentials.homeserver,
      session.helperAccessToken,
      session.seedRoomId,
      spoilerBody,
      `QA spoiler keep <span data-mx-spoiler="reason">${spoilerSecret}</span> hidden`,
      `qa-spoiler-${safeTimestamp()}`
    );
    await waitForElementCountGreaterThan(
      session.browser,
      ".message-spoiler",
      baselineSpoilers,
      timeoutMs,
      "local GUI spoiler render"
    );
    const leakedBeforeReveal = await session.browser.execute(
      (secret) => (document.body.textContent ?? "").includes(secret),
      spoilerSecret
    );
    if (leakedBeforeReveal) {
      throw new Error("local GUI spoiler text was visible before reveal");
    }
    const spoilerButton = await session.browser.$('.message-spoiler[data-revealed="false"]');
    await spoilerButton.waitForDisplayed({ timeout: timeoutMs });
    await spoilerButton.click();
    await waitForDocumentText(
      session.browser,
      [spoilerSecret],
      timeoutMs,
      "local GUI spoiler reveal"
    );
    console.log("gui_local_spoiler=ok");

    await recordLocalGuiEvidence(session);
    console.log("gui_local_message_types=ok");
  } finally {
    await cleanupLocalGuiScenario(session);
  }
}
