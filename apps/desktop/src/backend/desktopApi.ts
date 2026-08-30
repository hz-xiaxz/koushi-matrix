import type { RequestId, TimelineKey } from "../domain/coreEvents";
import type {
  ActivityMarkReadTarget,
  ActivityTab,
  AttachmentFilter,
  AttachmentSort,
  ComposerDocument,
  ComposerDraftAcceptanceResponse,
  ComposerDraftRevision,
  ComposerKeyEvent,
  ComposerResolvedAction,
  ComposerResolverOptions,
  ComposerSurface,
  ComposerTarget,
  CreateRoomRequest,
  DesktopSnapshot,
  DirectoryQuery,
  DisplayPlatform,
  EncryptionDebugOperationOutcome,
  FilesViewScope,
  InviteScopeSelection,
  MentionSurface,
  OidcAuthorization,
  PresenceKind,
  RoomKeyReshareOutcome,
  RoomListFilter,
  RoomListProjection,
  RoomModerationAction,
  RoomNotificationMode,
  RoomSettingChange,
  RoomTagKind,
  SavedSessionInfo,
  SearchScopeKind,
  SessionStatusRefreshTrigger,
  SettingsPatch,
  StageUploadBytesRequestItem,
  StagedUploadCompressionChoice,
  StagedUploadOutputSelection,
  SubmissionResponse,
  ThreadOpenIntent,
  ThreadsListScope,
} from "../domain/types";
import type { DiagnosticLogSnapshot } from "../domain/diagnostics";
import type {
  ComposerDraftLeaseSnapshot,
  ComposerDraftScope
} from "../domain/composerDraftLifecycle";

export type ViewportSyncTrigger =
  | "page_load"
  | "resized"
  | "scale_factor_changed"
  | "density_commit"
  | "browser_resize";
export type ViewportSyncDensity = "compact" | "default" | "comfortable";

export interface ViewportSyncSize {
  width: number;
  height: number;
}

export interface ViewportSyncRect extends ViewportSyncSize {
  top: number;
  left: number;
}

export interface ViewportSyncObservation {
  trigger: ViewportSyncTrigger;
  density: ViewportSyncDensity;
  window: ViewportSyncSize;
  document: ViewportSyncSize;
  visualViewport: {
    present: boolean;
    width: number;
    height: number;
    offsetLeft: number;
    offsetTop: number;
  };
  body: ViewportSyncRect;
  root: ViewportSyncRect;
}

export interface ViewportSyncReceipt {
  generation: number;
  trigger: ViewportSyncTrigger;
  density: ViewportSyncDensity | null;
  nativeSupport: "supported" | "unsupported";
  decision: "in_sync" | "repair_to_parent_bounds" | "unsupported";
  nativeAligned: boolean;
  nativeOriginAligned: boolean;
  nativeSizeAligned: boolean;
  domAligned: boolean;
  domJsAligned: boolean;
  domRootAligned: boolean;
  parent: ViewportSyncRect | null;
  webview: ViewportSyncRect | null;
}

export interface DesktopApi {
  getSnapshot(): Promise<DesktopSnapshot>;
  getDiagnosticSnapshot(): Promise<DiagnosticLogSnapshot>;
  observeViewportSync(observation: ViewportSyncObservation): Promise<ViewportSyncReceipt>;
  discoverLoginMethods(homeserver: string): Promise<DesktopSnapshot>;
  startOidcLogin(homeserver: string): Promise<OidcAuthorization>;
  completeOidcLogin(homeserver: string, callbackUrl: string): Promise<DesktopSnapshot>;
  submitLogin(
    homeserver: string,
    username: string,
    password: string,
    deviceDisplayName: string,
    platform: DisplayPlatform
  ): Promise<DesktopSnapshot>;
  submitSoftLogoutReauth(password: string): Promise<DesktopSnapshot>;
  listSavedSessions(): Promise<SavedSessionInfo[]>;
  switchAccount(session: SavedSessionInfo): Promise<DesktopSnapshot>;
  retrySlidingSyncCapability(): Promise<DesktopSnapshot>;
  changeHomeserver(): Promise<DesktopSnapshot>;
  logout(): Promise<DesktopSnapshot>;
  submitRecovery(secret: string): Promise<DesktopSnapshot>;
  /** Dedicated Secure Backup commands. */
  recoverSecureBackup: (secret: string) => Promise<DesktopSnapshot>;
  setupSecureBackup: (
    passphrase: string | null,
    recoveryKeyDestinationPath: string | null
  ) => Promise<DesktopSnapshot>;
  reenableSecureBackup: (
    passphrase: string | null,
    recoveryKeyDestinationPath: string | null
  ) => Promise<DesktopSnapshot>;
  retrySecureBackupInspection: () => Promise<DesktopSnapshot>;
  startDeviceCleanup(): Promise<DesktopSnapshot>;
  submitDeviceCleanupUia(flowId: number, password: string): Promise<DesktopSnapshot>;
  eraseLocalDataAnyway(): Promise<DesktopSnapshot>;
  restartSync(): Promise<DesktopSnapshot>;
  updateSettings(patch: SettingsPatch): Promise<DesktopSnapshot>;
  rebuildSearchIndex(): Promise<DesktopSnapshot>;
  setRoomUrlPreviewOverride(roomId: string, enabled: boolean): Promise<DesktopSnapshot>;
  selectRoomListFilter(filter: RoomListFilter): Promise<DesktopSnapshot>;
  markRoomAsRead(roomId: string, eventId: string): Promise<DesktopSnapshot>;
  markRoomAsUnread(roomId: string, unread: boolean): Promise<DesktopSnapshot>;
  setRoomNotificationMode(roomId: string, mode: RoomNotificationMode): Promise<DesktopSnapshot>;
  refreshCurrentSessionStatus(trigger: SessionStatusRefreshTrigger): Promise<DesktopSnapshot>;
  submitAccountManagementUia(flowId: number, password: string): Promise<DesktopSnapshot>;
  loadAccountManagementCapabilities(): Promise<DesktopSnapshot>;
  changePassword(newPassword: string): Promise<DesktopSnapshot>;
  deactivateAccount(eraseData: boolean): Promise<DesktopSnapshot>;
  probeLocalEncryptionHealth(): Promise<DesktopSnapshot>;
  resetLocalData(): Promise<DesktopSnapshot>;
  bootstrapCrossSigning(): Promise<DesktopSnapshot>;
  enableKeyBackup(): Promise<DesktopSnapshot>;
  exportRoomKeys(destinationPath: string, passphrase: string): Promise<DesktopSnapshot>;
  importRoomKeys(sourcePath: string, passphrase: string): Promise<DesktopSnapshot>;
  bootstrapSecureBackup(
    passphrase: string | null,
    recoveryKeyDestinationPath: string | null
  ): Promise<DesktopSnapshot>;
  changeSecureBackupPassphrase(
    oldSecret: string,
    newPassphrase: string,
    recoveryKeyDestinationPath: string | null
  ): Promise<DesktopSnapshot>;
  acceptVerification(flowId: number): Promise<DesktopSnapshot>;
  startOwnUserSas(): Promise<DesktopSnapshot>;
  retryCurrentDeviceTrustDiscovery(): Promise<DesktopSnapshot>;
  mismatchSasVerification(flowId: number): Promise<DesktopSnapshot>;
  startSessionBootstrap(passphrase: string | null, recoveryKeyDestinationPath: string): Promise<DesktopSnapshot>;
  confirmSessionBootstrapSaved(flowId: number): Promise<DesktopSnapshot>;
  confirmSasVerification(flowId: number): Promise<DesktopSnapshot>;
  cancelVerification(flowId: number): Promise<DesktopSnapshot>;
  resetIdentity(): Promise<DesktopSnapshot>;
  cancelIdentityReset(flowId: number): Promise<DesktopSnapshot>;
  submitIdentityResetPassword(flowId: number, password: string): Promise<DesktopSnapshot>;
  submitIdentityResetOAuth(flowId: number): Promise<DesktopSnapshot>;
  resolveComposerKeyAction(
    surface: ComposerSurface,
    keyEvent: ComposerKeyEvent,
    options: ComposerResolverOptions
  ): Promise<ComposerResolvedAction>;
  selectSpace(spaceId: string | null): Promise<DesktopSnapshot>;
  reorderSpaces(spaceIds: string[]): Promise<DesktopSnapshot>;
  selectRoom(roomId: string): Promise<DesktopSnapshot>;
  openActivityEvent(roomId: string, eventId: string): Promise<DesktopSnapshot>;
  openPinnedEvent(roomId: string, eventId: string): Promise<DesktopSnapshot>;
  selectSearchResult(roomId: string, eventId: string): Promise<DesktopSnapshot>;
  acknowledgeTimelineProjection(
    projectionRequestId: RequestId,
    key: TimelineKey,
    generation: number,
    itemCount: number,
    targetPresent: boolean
  ): Promise<void>;
  acknowledgeTimelineBatchRendered(
    key: TimelineKey,
    actorGeneration: number,
    timelineGeneration: number,
    repairGeneration: number,
    batchId: number
  ): Promise<void>;
  openTimelineAtTimestamp(roomId: string, timestampMs: number): Promise<DesktopSnapshot>;
  closeFocusedContext(): Promise<DesktopSnapshot>;
  closeSearch(): Promise<DesktopSnapshot>;
  beginComposerDraftRendererGeneration(): Promise<string>;
  acquireComposerDraftLease(
    scope: ComposerDraftScope,
    rendererGeneration: string
  ): Promise<ComposerDraftLeaseSnapshot>;
  releaseComposerDraftLease(leaseId: string, rendererGeneration: string): Promise<void>;
  sendText(
    account: ComposerDraftAccountOwner,
    leaseId: string,
    rendererGeneration: string,
    submissionId: string,
    roomId: string,
    document: ComposerDocument,
    draftRevision?: ComposerDraftRevision
  ): Promise<SubmissionResponse>;
  scheduleSend(
    account: ComposerDraftAccountOwner,
    leaseId: string,
    rendererGeneration: string,
    target: ComposerTarget,
    body: string,
    sendAtMs: number,
    draftRevision: ComposerDraftRevision
  ): Promise<ComposerDraftAcceptanceResponse>;
  stageUploadBytes(
    target: ComposerTarget,
    items: StageUploadBytesRequestItem[]
  ): Promise<DesktopSnapshot>;
  selectStagedUploadOutput(
    target: ComposerTarget,
    stagedId: string,
    selection: StagedUploadOutputSelection
  ): Promise<DesktopSnapshot>;
  retryStagedUploadPreparation(target: ComposerTarget, stagedId: string): Promise<DesktopSnapshot>;
  useOriginalStagedUpload(target: ComposerTarget, stagedId: string): Promise<DesktopSnapshot>;
  preparedUploadPreview(
    target: ComposerTarget,
    stagedId: string,
    variantId: string
  ): Promise<number[]>;
  sendPreparedUploads(
    account: ComposerDraftAccountOwner,
    leaseId: string,
    rendererGeneration: string,
    target: ComposerTarget,
    draftRevision: ComposerDraftRevision
  ): Promise<ComposerDraftAcceptanceResponse>;
  updateStagedUploadCaption(
    target: ComposerTarget,
    stagedId: string,
    document: ComposerDocument | null
  ): Promise<DesktopSnapshot>;
  updateStagedUploadCompression(
    target: ComposerTarget,
    stagedId: string,
    compressionChoice: StagedUploadCompressionChoice
  ): Promise<DesktopSnapshot>;
  clearUploadStaging(target: ComposerTarget): Promise<DesktopSnapshot>;
  cancelScheduledSend(scheduledId: string): Promise<DesktopSnapshot>;
  rescheduleScheduledSend(
    scheduledId: string,
    body: string,
    sendAtMs: number
  ): Promise<DesktopSnapshot>;
  retrySend(roomId: string, transactionId: string): Promise<DesktopSnapshot>;
  cancelSend(roomId: string, transactionId: string): Promise<DesktopSnapshot>;
  sendReaction(roomId: string, eventId: string, reactionKey: string): Promise<DesktopSnapshot>;
  redactReaction(
    roomId: string,
    eventId: string,
    reactionKey: string,
    reactionEventId: string
  ): Promise<DesktopSnapshot>;
  sendReadReceipt(roomId: string, eventId: string, threadRootEventId?: string | null): Promise<void>;
  setFullyRead(roomId: string, eventId: string): Promise<void>;
  setTyping(roomId: string, isTyping: boolean): Promise<void>;
  setPresence(presence: PresenceKind): Promise<DesktopSnapshot>;
  setDisplayName(displayName: string | null): Promise<DesktopSnapshot>;
  setLocalUserAlias(userId: string, alias: string | null): Promise<DesktopSnapshot>;
  ignoreUser(userId: string): Promise<DesktopSnapshot>;
  unignoreUser(userId: string): Promise<DesktopSnapshot>;
  reportUser(userId: string, reason: string): Promise<DesktopSnapshot>;
  reportContent(roomId: string, eventId: string, reason: string): Promise<DesktopSnapshot>;
  reportRoom(roomId: string, reason: string): Promise<DesktopSnapshot>;
  setAvatar(mimeType: string, bytes: number[]): Promise<DesktopSnapshot>;
  editMessage(
    roomId: string,
    eventId: string,
    document: ComposerDocument
  ): Promise<DesktopSnapshot>;
  redactMessage(roomId: string, eventId: string): Promise<DesktopSnapshot>;
  loadMessageSource(roomId: string, eventId: string): Promise<DesktopSnapshot>;
  requestRoomKey(
    roomId: string,
    eventId: string,
    origin?: "user" | "automatic",
    timelineKey?: TimelineKey
  ): Promise<DesktopSnapshot>;
  requestLateDecryption(
    roomId: string,
    timelineKey?: TimelineKey
  ): Promise<DesktopSnapshot>;
  forwardMessage(
    roomId: string,
    sourceEventId: string,
    destinationRoomId: string
  ): Promise<DesktopSnapshot>;
  loadLinkPreviews(roomId: string, eventId: string): Promise<DesktopSnapshot>;
  hideLinkPreview(roomId: string, eventId: string): Promise<DesktopSnapshot>;
  leaveRoom(roomId: string): Promise<DesktopSnapshot>;
  forgetRoom(roomId: string): Promise<DesktopSnapshot>;
  setRoomTag(roomId: string, tag: RoomTagKind, order?: number | null): Promise<DesktopSnapshot>;
  removeRoomTag(roomId: string, tag: RoomTagKind): Promise<DesktopSnapshot>;
  pinEvent(roomId: string, eventId: string): Promise<DesktopSnapshot>;
  unpinEvent(roomId: string, eventId: string): Promise<DesktopSnapshot>;
  reshareRoomKey(roomId: string): Promise<RoomKeyReshareOutcome>;
  forceNewOutboundSession(roomId: string): Promise<EncryptionDebugOperationOutcome>;
  shareIndex0RoomKey(roomId: string): Promise<EncryptionDebugOperationOutcome>;
  resendIndex0RoomKey(roomId: string): Promise<EncryptionDebugOperationOutcome>;
  openActivity(): Promise<DesktopSnapshot>;
  closeActivity(): Promise<DesktopSnapshot>;
  setActivityTab(tab: ActivityTab): Promise<DesktopSnapshot>;
  paginateActivity(tab: ActivityTab, cursor?: string | null): Promise<DesktopSnapshot>;
  retryActivityResolution(): Promise<DesktopSnapshot>;
  markActivityRead(target: ActivityMarkReadTarget): Promise<DesktopSnapshot>;
  setComposerDraft(
    account: ComposerDraftAccountOwner,
    leaseId: string,
    rendererGeneration: string,
    roomId: string,
    document: ComposerDocument,
    revision: ComposerDraftRevision
  ): Promise<DesktopSnapshot>;
  openThread(
    roomId: string,
    rootEventId: string,
    intent: ThreadOpenIntent
  ): Promise<DesktopSnapshot>;
  closeThread(): Promise<DesktopSnapshot>;
  openThreadsList(scope: ThreadsListScope): Promise<DesktopSnapshot>;
  closeThreadsList(): Promise<DesktopSnapshot>;
  paginateThreadsList(scope: ThreadsListScope): Promise<DesktopSnapshot>;
  openFilesView(scope: FilesViewScope, filter: AttachmentFilter, sort: AttachmentSort): Promise<DesktopSnapshot>;
  closeFilesView(): Promise<DesktopSnapshot>;
  setThreadComposerDraft(
    account: ComposerDraftAccountOwner,
    leaseId: string,
    rendererGeneration: string,
    roomId: string,
    rootEventId: string,
    document: ComposerDocument,
    revision: ComposerDraftRevision
  ): Promise<DesktopSnapshot>;
  sendThreadReply(
    account: ComposerDraftAccountOwner,
    leaseId: string,
    rendererGeneration: string,
    submissionId: string,
    roomId: string,
    rootEventId: string,
    document: ComposerDocument,
    draftRevision?: ComposerDraftRevision
  ): Promise<SubmissionResponse>;
  submitSearch(query: string, scope: SearchScopeKind): Promise<DesktopSnapshot>;
  queryDirectory(query: DirectoryQuery): Promise<DesktopSnapshot>;
  joinDirectoryRoom(roomIdOrAlias: string, viaServers?: string[]): Promise<DesktopSnapshot>;
  previewJoinTarget(roomIdOrAlias: string, viaServers?: string[]): Promise<DesktopSnapshot>;
  dismissDirectoryPreview(): Promise<DesktopSnapshot>;
  joinRoom(roomId: string): Promise<DesktopSnapshot>;
  loadRoomSettings(roomId: string): Promise<DesktopSnapshot>;
  loadSpaceMembers(spaceId: string, generation: number): Promise<DesktopSnapshot>;
  inviteUserToSpace(
    spaceId: string,
    userId: string,
    generation: number
  ): Promise<DesktopSnapshot>;
  cancelSpaceInvite(
    spaceId: string,
    userId: string,
    generation: number
  ): Promise<DesktopSnapshot>;
  queryMentionCandidates(
    roomId: string,
    surface: MentionSurface,
    query: string
  ): Promise<void>;
  repairRoomTimeline(roomId: string): Promise<DesktopSnapshot>;
  updateRoomSetting(roomId: string, change: RoomSettingChange): Promise<DesktopSnapshot>;
  moderateRoomMember(
    roomId: string,
    targetUserId: string,
    action: RoomModerationAction,
    reason?: string | null
  ): Promise<DesktopSnapshot>;
  updateRoomMemberRole(
    roomId: string,
    targetUserId: string,
    powerLevel: number
  ): Promise<DesktopSnapshot>;
  updateSpaceMemberRole(
    spaceId: string,
    userId: string,
    generation: number,
    expectedPowerLevelsRevision: string | null,
    expectedPowerLevel: number,
    powerLevel: number,
    confirmed: boolean
  ): Promise<DesktopSnapshot>;
  createRoom(request: CreateRoomRequest): Promise<DesktopSnapshot>;
  createSpace(name: string): Promise<DesktopSnapshot>;
  setSpaceChild(spaceId: string, childRoomId: string, viaServer: string): Promise<DesktopSnapshot>;
  acceptInvite(roomId: string): Promise<DesktopSnapshot>;
  declineInvite(roomId: string): Promise<DesktopSnapshot>;
  startDirectMessage(userId: string): Promise<DesktopSnapshot>;
  inviteUser(roomId: string, userId: string): Promise<DesktopSnapshot>;
  openInviteWorkflow(roomId: string): Promise<DesktopSnapshot>;
  closeInviteWorkflow(): Promise<DesktopSnapshot>;
  searchInviteTargets(roomId: string, query: string): Promise<DesktopSnapshot>;
  setInviteScope(roomId: string, scope: InviteScopeSelection): Promise<DesktopSnapshot>;
  selectInviteTarget(roomId: string, userId: string): Promise<DesktopSnapshot>;
  removeInviteTarget(userId: string): Promise<DesktopSnapshot>;
  inviteTargets(
    roomId: string,
    userIds: string[],
    scope: InviteScopeSelection
  ): Promise<DesktopSnapshot>;
  setComposerReplyTarget(roomId: string, eventId: string): Promise<DesktopSnapshot>;
  cancelComposerReply(): Promise<DesktopSnapshot>;
  sendReply(
    account: ComposerDraftAccountOwner,
    leaseId: string,
    rendererGeneration: string,
    submissionId: string,
    roomId: string,
    inReplyToEventId: string,
    document: ComposerDocument,
    draftRevision?: ComposerDraftRevision
  ): Promise<SubmissionResponse>;
  setRoomListProjection(projection: RoomListProjection): void;
  startRoomCrawl(roomId: string): Promise<DesktopSnapshot>;
  stopRoomCrawl(roomId: string): Promise<DesktopSnapshot>;
}

export interface ComposerDraftAccountOwner {
  homeserver: string;
  userId: string;
  deviceId: string;
}
