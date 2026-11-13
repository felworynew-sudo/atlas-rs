import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import App from './App';
import { BASE } from './base';
import { SessionProvider } from './session';
import './styles.css';
import { applyTheme, savedTheme } from './pages/Appearance';

applyTheme(savedTheme());

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <BrowserRouter basename={BASE || undefined}>
      <SessionProvider>
        <App />
      </SessionProvider>
    </BrowserRouter>
  </StrictMode>,
);
