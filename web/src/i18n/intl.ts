// 界面数字/日期格式化 locale:跟随当前 i18n 语言,而不是浏览器语言。
import i18n from './index';

export const currentIntlLocale = (): string => (i18n.language === 'zh' ? 'zh-CN' : 'en-US');
