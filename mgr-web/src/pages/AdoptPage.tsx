// AdoptPage - import-an-existing-stack wizard (Phase 5, design §3.8).
//
// Registers an EXTERNAL compose stack (e.g. the repo's own
// docker-compose.yml started via `make up`) under mgr WITHOUT recreating
// anything: the backend connects the stack's gateway/app containers onto
// aio-mgr-net with the sbx-<name> aliases and adds the subdomain routes.
// Management is read-only by design - status / start / stop / routes; env +
// resource edits are rejected (the compose file is not ours), and "delete"
// only un-registers (containers and volumes stay).
//
// POST /api/sandboxes/adopt is synchronous (no JobView); success returns to
// the list. NAME_RE is shared with CreatePage (same slug contract as
// routes.rs validate_name).
//
// S7 (prototype redesign): inputs move to the component layer (.input +
// .field.invalid/.err pattern from CreatePage); flow unchanged.

import { useState } from "react";

import { adoptSandbox } from "../api";
import { t, type Lang } from "../i18n";
import { Icon } from "../icons";
import { NAME_RE } from "./CreatePage";

interface Props {
  lang: Lang;
  onCancel: () => void;
  onAdopted: () => void;
}

export function AdoptPage({ lang, onCancel, onAdopted }: Props): JSX.Element {
  const [name, setName] = useState("");
  const [composePath, setComposePath] = useState("docker-compose.yml");
  const [advanced, setAdvanced] = useState(false);
  const [gatewayService, setGatewayService] = useState("gateway");
  const [appService, setAppService] = useState("app");
  const [msg, setMsg] = useState<{ kind: "err" | "ok"; text: string } | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const nameErr = name === "" ? "" : NAME_RE.test(name) ? "" : t(lang, "wzNameErr");
  const pathErr = composePath.trim() === "" ? t(lang, "adPathErr") : "";
  // Blank service names (advanced section open) would probe for service "".
  const svcErr =
    advanced && (gatewayService.trim() === "" || appService.trim() === "")
      ? t(lang, "adSvcErr")
      : "";
  const canSubmit =
    NAME_RE.test(name) && pathErr === "" && svcErr === "" && !submitting;

  const submit = async () => {
    if (!canSubmit) return;
    setSubmitting(true);
    setMsg(null);
    try {
      await adoptSandbox({
        name,
        compose_path: composePath.trim(),
        // Blank inputs fall back to the backend defaults.
        gateway_service: gatewayService.trim() || "gateway",
        app_service: appService.trim() || "app",
      });
      onAdopted();
    } catch (e) {
      setSubmitting(false);
      const text = e instanceof Error ? e.message : String(e);
      setMsg({
        kind: "err",
        text: text.includes("already exists") ? t(lang, "wzNameTaken") : text,
      });
    }
  };

  return (
    <div className="page">
      <div className="page-head">
        <h1>{t(lang, "adTitle")}</h1>
        <div className="page-actions">
          <button className="btn btn-secondary" onClick={onCancel}>
            {t(lang, "cancel")}
          </button>
        </div>
      </div>
      <div className="wizard">
        <div className={`field${nameErr !== "" ? " invalid" : ""}`} style={{ maxWidth: 480 }}>
          <label>{t(lang, "wzName")}</label>
          <input
            className="input mono"
            value={name}
            placeholder={t(lang, "wzNamePh")}
            aria-invalid={nameErr !== ""}
            autoFocus
            onChange={(e) => setName(e.target.value.trim())}
          />
          <span className="hint">{t(lang, "wzNameHint")}</span>
          <span className="err">{t(lang, "wzNameErr")}</span>
        </div>

        <div className={`field${pathErr !== "" ? " invalid" : ""}`} style={{ maxWidth: 480 }}>
          <label>{t(lang, "adComposePath")}</label>
          <input
            className="input mono"
            value={composePath}
            placeholder={t(lang, "adPathPh")}
            aria-invalid={pathErr !== ""}
            onChange={(e) => setComposePath(e.target.value)}
          />
          <span className="hint">{t(lang, "adPathHint")}</span>
          <span className="err">{t(lang, "adPathErr")}</span>
        </div>

        <div>
          <button
            className="btn btn-ghost btn-sm"
            aria-expanded={advanced}
            onClick={() => setAdvanced((a) => !a)}
          >
            <Icon name="chev-down" />
            {t(lang, "adAdvanced")}
          </button>
        </div>
        {advanced && (
          <div className="field-row" style={{ maxWidth: 480 }}>
            <div className={`field${svcErr !== "" && gatewayService.trim() === "" ? " invalid" : ""}`}>
              <label>{t(lang, "adGwService")}</label>
              <input
                className="input mono"
                value={gatewayService}
                aria-invalid={svcErr !== "" && gatewayService.trim() === ""}
                onChange={(e) => setGatewayService(e.target.value)}
              />
              <span className="hint">{t(lang, "adSvcHint")}</span>
              <span className="err">{t(lang, "adSvcErr")}</span>
            </div>
            <div className={`field${svcErr !== "" && appService.trim() === "" ? " invalid" : ""}`}>
              <label>{t(lang, "adAppService")}</label>
              <input
                className="input mono"
                value={appService}
                aria-invalid={svcErr !== "" && appService.trim() === ""}
                onChange={(e) => setAppService(e.target.value)}
              />
              <span className="err">{t(lang, "adSvcErr")}</span>
            </div>
          </div>
        )}

        <div className="status">{t(lang, "adNotice")}</div>

        <div className="dialog-actions">
          <button className="btn btn-primary" disabled={!canSubmit} onClick={() => void submit()}>
            {submitting ? t(lang, "adSubmitting") : t(lang, "adSubmit")}
          </button>
          {msg && (
            <span className={`wizard-msg ${msg.kind === "err" ? "err" : "ok"}`}>{msg.text}</span>
          )}
        </div>
      </div>
    </div>
  );
}
