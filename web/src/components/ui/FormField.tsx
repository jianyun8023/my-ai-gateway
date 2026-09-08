import { Select, TextInput, Textarea, type SelectProps } from '@mantine/core';
import { useId, type AriaAttributes, type InputHTMLAttributes, type TextareaHTMLAttributes } from 'react';

interface FieldBaseProps {
  label: string;
  hint?: string;
  error?: string;
  className?: string;
}

// Mantine owns the wrapper; preserve caller descriptions in addition to the
// generated hint/error references through the input slot's attributes.
function useFieldProps({ hint, error, id, describedBy, invalid }: {
  hint?: string; error?: string; id?: string; describedBy?: string; invalid?: AriaAttributes['aria-invalid'];
}) {
  const generatedId = useId();
  const fieldId = id ?? generatedId;
  const hintId = hint ? fieldId + '-hint' : undefined;
  const errorId = error ? fieldId + '-error' : undefined;
  return {
    id: fieldId,
    description: hint,
    error,
    descriptionProps: { id: hintId },
    errorProps: { id: errorId, role: 'alert' },
    attributes: { input: {
      'aria-describedby': [describedBy, hintId, errorId].filter(Boolean).join(' ') || undefined,
      'aria-invalid': error ? true : invalid,
    } },
  };
}

type FieldAttributes<T> = Omit<T, 'size' | 'color'>;
export function TextField({ hint, error, id, 'aria-describedby': describedBy, 'aria-invalid': invalid, ...props }: FieldBaseProps & FieldAttributes<InputHTMLAttributes<HTMLInputElement>>) {
  const field = useFieldProps({ hint, error, id, describedBy, invalid });
  return <TextInput {...props} {...field} />;
}

type SelectFieldProps = FieldBaseProps & Omit<FieldAttributes<SelectProps<string>>,
  'allowDeselect' | 'children' | 'data' | 'description' | 'error' | 'label' | 'onChange'> & {
  data: NonNullable<SelectProps<string>['data']>;
  onChange?: (value: string) => void;
};

export function SelectField({ hint, error, id, 'aria-describedby': describedBy, 'aria-invalid': invalid,
  data, onChange, ...props }: SelectFieldProps) {
  const field = useFieldProps({ hint, error, id, describedBy, invalid });
  return <Select {...props} data={data} {...field} allowDeselect={false} onChange={(value) => {
    if (value !== null) onChange?.(value);
  }} />;
}

export function TextAreaField({ hint, error, id, 'aria-describedby': describedBy, 'aria-invalid': invalid, ...props }: FieldBaseProps & FieldAttributes<TextareaHTMLAttributes<HTMLTextAreaElement>>) {
  const field = useFieldProps({ hint, error, id, describedBy, invalid });
  return <Textarea {...props} {...field} />;
}
