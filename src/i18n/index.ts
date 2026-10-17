import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import enCommon from "./locales/en/common.json";
import esCommon from "./locales/es/common.json";

/** Source locale: all keys and source strings are authored in English. */
export const SOURCE_LOCALE = "en";
/** Default display locale for the CINEPOL community. */
export const DEFAULT_LOCALE = "es";

export const resources = {
  en: { common: enCommon },
  es: { common: esCommon },
} as const;

void i18n.use(initReactI18next).init({
  resources,
  lng: DEFAULT_LOCALE,
  fallbackLng: SOURCE_LOCALE,
  defaultNS: "common",
  ns: ["common"],
  initAsync: false,
  interpolation: { escapeValue: false },
});

i18n.on("languageChanged", (language) => {
  document.documentElement.lang = language;
});

export { i18n };
