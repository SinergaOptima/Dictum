"use client";

type GuidedTuneCardProps = {
  kicker: string;
  title: string;
  readyLabel: string;
  pendingLabel: string;
  completed: boolean;
  note: string;
  progressPct: number;
  stepLabel: string;
  instruction: string;
  sentence: string | null;
  className?: string;
};

export function GuidedTuneCard({
  kicker,
  title,
  readyLabel,
  pendingLabel,
  completed,
  note,
  progressPct,
  stepLabel,
  instruction,
  sentence,
  className,
}: GuidedTuneCardProps) {
  const classes = ["guided-tune-card", className].filter(Boolean).join(" ");

  return (
    <div className={classes}>
      <div className="guided-tune-head">
        <div>
          <span className="settings-kicker">{kicker}</span>
          <h3>{title}</h3>
        </div>
        <span className={`guided-tune-badge ${completed ? "is-ready" : ""}`}>
          {completed ? readyLabel : pendingLabel}
        </span>
      </div>
      <p className="settings-note">{note}</p>
      <div className="guided-tune-progress" aria-hidden="true">
        <div style={{ width: `${progressPct}%` }} />
      </div>
      <div className="guided-tune-copy">
        <strong>{stepLabel}</strong>
        <span>{instruction}</span>
        {sentence && <em>"{sentence}"</em>}
      </div>
    </div>
  );
}
