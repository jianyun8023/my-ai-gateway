import type { GatewayProtocol } from '@/admin-api';

export const PROTOCOL_LABELS: Record<GatewayProtocol, string> = {
  openai_chat_completions: 'Chat Completions',
  openai_responses: 'Responses',
  anthropic_messages: 'Messages',
};
