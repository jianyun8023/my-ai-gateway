import { useId, type AriaAttributes, type InputHTMLAttributes, type SelectHTMLAttributes, type TextareaHTMLAttributes, type ReactNode } from 'react';
import styles from './ConsolePrimitives.module.scss';

interface FieldBaseProps {
  label: string;
  hint?: string;
  error?: string;
  className?: string;
}

// One label/hint/error contract for every native control. Caller descriptions
// are preserved alongside the generated IDs (including inside Portal forms).
function FieldFrame({ label, hint, error, className = '', id, describedBy, invalid, children }: FieldBaseProps & {
  id?: string;
  describedBy?: string;
  invalid?: AriaAttributes['aria-invalid'];
  children: (attributes: { id: string; 'aria-describedby'?: string; 'aria-invalid'?: AriaAttributes['aria-invalid'] }) => ReactNode;
}) {
  const generatedId = useId();
  const fieldId = id ?? generatedId;
  const hintId = hint ? `${fieldId}-hint` : undefined;
  const errorId = error ? `${fieldId}-error` : undefined;
  return (
    <div className={`${styles.field} ${className}`.trim()}>
      <label htmlFor={fieldId}>{label}</label>
      {children({ id: fieldId, 'aria-describedby': [describedBy, hintId, errorId].filter(Boolean).join(' ') || undefined, 'aria-invalid': error ? true : invalid })}
      {hint && <small id={hintId}>{hint}</small>}
      {error && <span id={errorId} role="alert">{error}</span>}
    </div>
  );
}

export function TextField({ label, hint, error, className, id, 'aria-describedby': describedBy, 'aria-invalid': invalid, ...props }: FieldBaseProps & InputHTMLAttributes<HTMLInputElement>) {
  return <FieldFrame {...{ label, hint, error, className, id, describedBy, invalid }}>{(attributes) => <input {...props} {...attributes} />}</FieldFrame>;
}

export function SelectField({ label, hint, error, className, id, 'aria-describedby': describedBy, 'aria-invalid': invalid, children, ...props }: FieldBaseProps & SelectHTMLAttributes<HTMLSelectElement>) {
  return <FieldFrame {...{ label, hint, error, className, id, describedBy, invalid }}>{(attributes) => <select {...props} {...attributes}>{children}</select>}</FieldFrame>;
}

export function TextAreaField({ label, hint, error, className, id, 'aria-describedby': describedBy, 'aria-invalid': invalid, ...props }: FieldBaseProps & TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <FieldFrame {...{ label, hint, error, className, id, describedBy, invalid }}>{(attributes) => <textarea {...props} {...attributes} />}</FieldFrame>;
}
