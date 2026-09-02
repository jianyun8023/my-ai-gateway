// 网关控制台 i18n 命名空间「console」(en / zh)注册入口。
// 用法:组件内 import '@/i18n/console' 后(或经 main.tsx 全局引入),useTranslation('console')。
import i18n from '../index';
import { common as enCommon } from './en/common';
import { errors as enErrors } from './en/errors';
import { shell as enShell } from './en/shell';
import { usage as enUsage } from './en/usage';
import { sources as enSources } from './en/sources';
import { models as enModels } from './en/models';
import { discovery as enDiscovery } from './en/discovery';
import { capabilities as enCapabilities } from './en/capabilities';
import { settings as enSettings } from './en/settings';
import { values as enValues } from './en/values';
import { common as zhCommon } from './zh/common';
import { errors as zhErrors } from './zh/errors';
import { shell as zhShell } from './zh/shell';
import { usage as zhUsage } from './zh/usage';
import { sources as zhSources } from './zh/sources';
import { models as zhModels } from './zh/models';
import { discovery as zhDiscovery } from './zh/discovery';
import { capabilities as zhCapabilities } from './zh/capabilities';
import { settings as zhSettings } from './zh/settings';
import { values as zhValues } from './zh/values';

const en = {
  common: enCommon,
  errors: enErrors,
  shell: enShell,
  usage: enUsage,
  sources: enSources,
  models: enModels,
  discovery: enDiscovery,
  capabilities: enCapabilities,
  settings: enSettings,
  values: enValues,
};

const zh = {
  common: zhCommon,
  errors: zhErrors,
  shell: zhShell,
  usage: zhUsage,
  sources: zhSources,
  models: zhModels,
  discovery: zhDiscovery,
  capabilities: zhCapabilities,
  settings: zhSettings,
  values: zhValues,
};

// 只在内存中聚合完毕后整体注册一次,避免多次 addResourceBundle 覆盖。
if (!i18n.hasResourceBundle('en', 'console')) {
  i18n.addResourceBundle('en', 'console', en);
  i18n.addResourceBundle('zh', 'console', zh);
}

export default i18n;
