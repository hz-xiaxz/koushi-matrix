import { type FormEvent, useRef, useState } from "react";
import { Check } from "lucide-react";

import { t } from "../../i18n/messages";
import { ImeSafeForm, SecureImeTextField } from "../ImeTextControl";

export function AccountManagementUiaForm({
  flowId,
  onSubmit
}: {
  flowId: number;
  onSubmit: (flowId: number, password: string) => void;
}) {
  const passwordInput = useRef<HTMLInputElement>(null);
  const [passwordFilled, setPasswordFilled] = useState(false);

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const password = passwordInput.current?.value ?? "";
    if (!password) {
      return;
    }
    onSubmit(flowId, password);
    if (passwordInput.current) {
      passwordInput.current.value = "";
    }
    setPasswordFilled(false);
  }

  return (
    <ImeSafeForm className="trust-auth-row" onSubmit={submit}>
      <label className="trust-password-field">
        <span>{t("auth.password")}</span>
        <SecureImeTextField
          autoComplete="current-password"
          ref={passwordInput}
          onInput={(event) => setPasswordFilled(event.currentTarget.value.length > 0)}
        />
      </label>
      <button className="trust-action-button primary" type="submit" disabled={!passwordFilled}>
        <Check size={14} />
        <span>{t("action.continue")}</span>
      </button>
    </ImeSafeForm>
  );
}
