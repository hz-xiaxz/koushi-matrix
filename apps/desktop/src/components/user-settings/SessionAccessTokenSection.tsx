import { useLayoutEffect, useRef, useState } from "react";
import { useActiveLocaleKey } from "../../i18n/activeLocaleContext";
import { t } from "../../i18n/messages";

export type SessionAccessTokenResponse = {
  access_token: string;
  homeserver: string;
  user_id: string;
  device_id: string;
};

export type SessionAccessTokenSectionProps = {
  /** Nonsecret session identity including the authoritative session generation. */
  sessionKey: string;
  homeserver: string;
  userId: string;
  deviceId: string;
  available: boolean;
  retrieve: () => Promise<SessionAccessTokenResponse>;
};

export function SessionAccessTokenSection(props: SessionAccessTokenSectionProps) {
  // Replacing the DOM owner also clears visible credentials and local statuses
  // before paint when the session changes, even if a request is still pending.
  const scope = JSON.stringify([props.sessionKey, props.homeserver, props.userId, props.deviceId, props.available]);
  return <ScopedAccessTokenSection key={scope} {...props} />;
}

function ScopedAccessTokenSection({ homeserver, userId, deviceId, available, retrieve }: SessionAccessTokenSectionProps) {
  useActiveLocaleKey();
  const tokenNode = useRef<HTMLElement>(null);
  const mounted = useRef(false);
  const request = useRef(0);
  const [revealed, setRevealed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<"loading" | "copied" | "failed" | "copyFailed" | null>(null);

  useLayoutEffect(() => {
    mounted.current = true;
    const node = tokenNode.current;
    return () => {
      mounted.current = false;
      request.current += 1;
      if (node) node.textContent = "";
    };
  }, []);

  function hide() {
    request.current += 1;
    if (tokenNode.current) tokenNode.current.textContent = "";
    setRevealed(false);
    setBusy(false);
    setStatus(null);
  }

  async function disclose(action: "reveal" | "copy") {
    if (!available || busy) return;
    const generation = ++request.current;
    const current = () => mounted.current && request.current === generation;
    setBusy(true);
    setStatus("loading");
    if (action === "reveal") setRevealed(true);
    let response: SessionAccessTokenResponse | null = null;
    try {
      // A reveal never supplies a later copy: read the live session for each action.
      response = await retrieve();
      if (!current()) return;
      if (response.user_id !== userId || response.device_id !== deviceId || response.homeserver !== homeserver || !response.access_token) {
        if (tokenNode.current) tokenNode.current.textContent = "";
        setRevealed(false);
        setStatus("failed");
        return;
      }
      if (action === "reveal") {
        if (tokenNode.current) tokenNode.current.textContent = response.access_token;
        setStatus(null);
      } else {
        try {
          await navigator.clipboard.writeText(response.access_token);
          if (current()) setStatus("copied");
        } catch {
          if (current()) setStatus("copyFailed");
        }
      }
    } catch {
      if (current()) {
        if (tokenNode.current) tokenNode.current.textContent = "";
        setRevealed(false);
        setStatus("failed");
      }
    } finally {
      response = null;
      if (current()) setBusy(false);
    }
  }

  return (
    <section className="settings-section" aria-label={t("accessToken.title")}>
      <div className="settings-section-heading"><h3>{t("accessToken.title")}</h3></div>
      <p className="profile-settings-hint">{t("accessToken.explanation")}</p>
      <p className="profile-settings-hint">{t("accessToken.clipboardWarning")}</p>
      <div className="settings-row">
        {!revealed ? <span aria-hidden="true">••••••••</span> : null}
        <code ref={tokenNode} hidden={!revealed} dir="ltr" style={{ overflowWrap: "anywhere", userSelect: "text" }} />
      </div>
      <div className="settings-row">
        <button className="trust-action-button" type="button" disabled={!available || (!revealed && busy)} onClick={() => revealed ? hide() : void disclose("reveal")}>
          {t(revealed ? "accessToken.hide" : "accessToken.reveal")}
        </button>
        <button className="trust-action-button" type="button" disabled={!available || busy} onClick={() => void disclose("copy")}>
          {t("accessToken.copy")}
        </button>
      </div>
      <p className="profile-settings-hint" role="status">
        {!available ? t("accessToken.unavailable") : status ? t(`accessToken.${status}`) : null}
      </p>
    </section>
  );
}
