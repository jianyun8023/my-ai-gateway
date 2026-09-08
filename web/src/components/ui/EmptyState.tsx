import { type ReactNode } from 'react';
import { IconInbox } from './icons';

interface EmptyStateProps {
  title: string;
  description?: string;
  action?: ReactNode;
  layout?: 'inline' | 'centered';
}

export function EmptyState({ title, description, action, layout = 'inline' }: EmptyStateProps) {
  return (
    <div className="empty-state" data-layout={layout}>
      <div className="empty-content">
        <div className="empty-icon" aria-hidden="true">
          <IconInbox size={20} />
        </div>
        <div>
          <div className="empty-title">{title}</div>
          {description && <div className="empty-desc">{description}</div>}
        </div>
      </div>
      {action && <div className="empty-action">{action}</div>}
    </div>
  );
}
