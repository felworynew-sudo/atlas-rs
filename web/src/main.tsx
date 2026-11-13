import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import App from './App';
import { BASE, EMBEDDED } from './base';
import { SessionProvider } from './session';
import './styles.css';
import { applyTheme, savedTheme } from './pages/Appearance';

applyTheme(savedTheme());
// Внутри дев-панели сайта студии — оформление сайта (синий и золотой); отдельно Атлас выглядит как раньше.
if (EMBEDDED || BASE === '/crm-app') document.documentElement.setAttribute('data-skin', 'wow');

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <BrowserRouter basename={BASE || undefined}>
      <SessionProvider>
        <App />
      </SessionProvider>
    </BrowserRouter>
  </StrictMode>,
);
