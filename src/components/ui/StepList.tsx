export interface Step {
  title: string;
  body?: string;
}

/**
 * Numbered steps (HowToJoin.astro). Use only where the order itself is the
 * information, e.g. the Microsoft sign-in sequence.
 */
export function StepList({ steps }: { steps: Step[] }) {
  return (
    <ol className="m-0 list-none border-t border-hairline p-0">
      {steps.map((step, index) => (
        <li key={step.title} className="flex gap-5 border-b border-hairline py-6 lg:gap-8">
          <span data-readout className="readout-md text-engrave">
            {String(index + 1).padStart(2, "0")}
          </span>
          <div>
            <p className="display-md m-0 text-silk">{step.title}</p>
            {step.body ? <p className="measure mt-2 mb-0 text-silk-dim">{step.body}</p> : null}
          </div>
        </li>
      ))}
    </ol>
  );
}
