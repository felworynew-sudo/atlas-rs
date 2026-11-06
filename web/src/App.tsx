import { Navigate, Route, Routes } from 'react-router-dom';
import Home from './pages/Home';
import Login from './pages/Login';
import NewSale from './pages/NewSale';
import ProductEdit from './pages/ProductEdit';
import Products from './pages/Products';
import SaleDetail from './pages/SaleDetail';
import SalesHistory from './pages/SalesHistory';
import { useSession } from './session';
import { Spinner } from './ui';

export default function App() {
  const s = useSession();
  if (!s.ready) return <Spinner />;
  if (!s.user) return <Login />;
  return (
    <div className="app">
      <Routes>
        <Route path="/" element={<Home />} />
        <Route path="/new-sale" element={<NewSale />} />
        <Route path="/sales-history" element={<SalesHistory />} />
        <Route path="/sale/:id" element={<SaleDetail />} />
        <Route path="/products" element={<Products />} />
        <Route path="/product/:id" element={<ProductEdit />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </div>
  );
}
