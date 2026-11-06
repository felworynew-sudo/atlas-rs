import plus from './icons/plus.svg?raw';
import clock from './icons/clock.svg?raw';
import fileText from './icons/file-text.svg?raw';
import trash from './icons/trash-2.svg?raw';
import bell from './icons/bell.svg?raw';
import clipboard from './icons/clipboard.svg?raw';
import pricetags from './icons/pricetags.svg?raw';
import cart from './icons/shopping-cart.svg?raw';
import chart from './icons/bar-chart-2.svg?raw';
import people from './icons/people.svg?raw';
import edit from './icons/edit-2.svg?raw';

// Иконки Eva (outline, 24×24). Недостающие нарисованы в том же стиле.
const wrap = (path: string, rule = '') => `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="#FFFFFF"><path ${rule} d="${path}"/></svg>`;

const GEAR =
  'M21 13.5v-3l-2.1-.5a7 7 0 0 0-.7-1.7l1.1-1.9-2.1-2.1-1.9 1.1a7 7 0 0 0-1.7-.7L13.5 3h-3l-.5 2.1a7 7 0 0 0-1.7.7L6.4 4.7 4.3 6.8l1.1 1.9a7 7 0 0 0-.7 1.7L3 10.5v3l2.1.5a7 7 0 0 0 .7 1.7l-1.1 1.9 2.1 2.1 1.9-1.1a7 7 0 0 0 1.7.7l.5 2.1h3l.5-2.1a7 7 0 0 0 1.7-.7l1.9 1.1 2.1-2.1-1.1-1.9a7 7 0 0 0 .7-1.7zM12 8.5a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7z';

const ICONS = {
  plus,
  clock,
  'file-text': fileText,
  trash,
  bell,
  clipboard,
  pricetags,
  cart,
  chart,
  people,
  edit,
  settings: wrap(GEAR, 'fill-rule="evenodd"'),
  back: wrap('M19 11H7.14l3.63-4.36a1 1 0 1 0-1.54-1.28l-5 6a1.19 1.19 0 0 0-.09.15c0 .05 0 .08-.07.13A1 1 0 0 0 4 12a1 1 0 0 0 .07.36c0 .05 0 .08.07.13a1.19 1.19 0 0 0 .09.15l5 6A1 1 0 0 0 10 19a1 1 0 0 0 .64-.23 1 1 0 0 0 .13-1.41L7.14 13H19a1 1 0 0 0 0-2z'),
  close: wrap('M13.41 12l4.3-4.29a1 1 0 1 0-1.42-1.42L12 10.59l-4.29-4.3a1 1 0 0 0-1.42 1.42l4.3 4.29-4.3 4.29a1 1 0 0 0 0 1.42 1 1 0 0 0 1.42 0l4.29-4.3 4.29 4.3a1 1 0 0 0 1.42 0 1 1 0 0 0 0-1.42z'),
  minus: wrap('M19 13H5a1 1 0 0 1 0-2h14a1 1 0 0 1 0 2z'),
  // грузовик (Приемки)
  truck: wrap('M20.5 9H17V6a1 1 0 0 0-1-1H3a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h1.1a3 3 0 0 0 5.8 0h4.2a3 3 0 0 0 5.8 0H22a1 1 0 0 0 1-1v-4.5a1 1 0 0 0-.3-.7l-1.5-1.5a1 1 0 0 0-.7-.3zM7 18.5a1 1 0 1 1 0-2 1 1 0 0 1 0 2zM4 7h11v8H9.9a3 3 0 0 0-5.8 0H4zm14 11.5a1 1 0 1 1 0-2 1 1 0 0 1 0 2zM17 11h2.6l.9.9V15h-.1a3 3 0 0 0-3.4-1.9z'),
} as const;

export type IconName = keyof typeof ICONS;

export function Icon({ name, size = 24 }: { name: IconName; size?: number }) {
  const svg = ICONS[name].replace(/fill="[^"]*"/, 'fill="currentColor"').replace(/width="\d+" height="\d+"/, `width="${size}" height="${size}"`);
  return <span className="icon" aria-hidden="true" dangerouslySetInnerHTML={{ __html: svg }} />;
}
