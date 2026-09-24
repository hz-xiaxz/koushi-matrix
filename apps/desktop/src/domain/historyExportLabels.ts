import { getActiveLocale, t } from "../i18n/messages";
import type { HistoryExportLabels } from "./types";

/** The page text of an export, resolved from the catalog in the active
 * locale. `t` leaves unfilled `{name}` placeholders in place; Rust fills them. */
export function historyExportLabels(): HistoryExportLabels {
  const raw = (id: Parameters<typeof t>[0]) => t(id);
  return {
    lang: getActiveLocale() === "ja" ? "ja" : "en",
    edited: raw("historyExport.page.edited"),
    inReplyTo: raw("historyExport.page.inReplyTo"),
    replyUnavailable: raw("historyExport.page.replyUnavailable"),
    threadReply: raw("historyExport.page.threadReply"),
    threadRootLink: raw("historyExport.page.threadRootLink"),
    redacted: raw("historyExport.page.redacted"),
    undecryptable: raw("historyExport.page.undecryptable"),
    notRetrieved: raw("historyExport.page.notRetrieved"),
    reactions: raw("historyExport.page.reactions"),
    timesInZone: raw("historyExport.page.timesInZone"),
    exportedAt: raw("historyExport.page.exportedAt"),
    rangeAll: raw("historyExport.page.rangeAll"),
    rangePeriod: raw("historyExport.page.rangePeriod"),
    roomsHeading: raw("historyExport.page.roomsHeading"),
    statusCompleted: raw("historyExport.page.statusCompleted"),
    statusSkipped: raw("historyExport.page.statusSkipped"),
    statusFailed: raw("historyExport.page.statusFailed"),
    statusPending: raw("historyExport.page.statusPending"),
    eventsCount: raw("historyExport.page.eventsCount"),
    attachmentsCount: raw("historyExport.page.attachmentsCount"),
    failedAttachmentsCount: raw("historyExport.page.failedAttachmentsCount"),
    stateJoined: raw("historyExport.page.stateJoined"),
    stateLeft: raw("historyExport.page.stateLeft"),
    stateInvited: raw("historyExport.page.stateInvited"),
    stateRemoved: raw("historyExport.page.stateRemoved"),
    stateBanned: raw("historyExport.page.stateBanned"),
    stateRenamed: raw("historyExport.page.stateRenamed"),
    stateTopic: raw("historyExport.page.stateTopic"),
    stateAvatar: raw("historyExport.page.stateAvatar"),
    stateOther: raw("historyExport.page.stateOther")
  };
}
