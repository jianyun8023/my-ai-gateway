// 网关控制台测试语言约定:
// 测试环境 navigator.language 通常为 en-US,i18n 初始化为 en;
// 需要断言具体文案(多数控制台测试断言简体中文)时,先调用 setTestLanguage('zh')。
import '@/i18n/console';
import i18n from '@/i18n';

export const setTestLanguage = async (language: 'en' | 'zh' = 'zh') => {
  await i18n.changeLanguage(language);
};
