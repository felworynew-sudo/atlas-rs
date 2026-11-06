import { Navigate, Route, Routes } from 'react-router-dom';
import Home from './pages/Home';
import Login from './pages/Login';
import NewSale from './pages/NewSale';
import ProductEdit from './pages/ProductEdit';
import Products from './pages/Products';
import SaleDetail from './pages/SaleDetail';
import RevisionForm from './pages/RevisionForm';
import Revisions from './pages/Revisions';
import SalesHistory from './pages/SalesHistory';
import StockCard from './pages/StockCard';
import Stocks from './pages/Stocks';
import Supplies from './pages/Supplies';
import SupplyForm from './pages/SupplyForm';
import SupplyOrderForm from './pages/SupplyOrderForm';
import SupplyOrders from './pages/SupplyOrders';
import WriteoffForm from './pages/WriteoffForm';
import Writeoffs from './pages/Writeoffs';
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
        <Route path="/stocks" element={<Stocks />} />
        <Route path="/stock/:id" element={<StockCard />} />
        <Route path="/supplies" element={<Supplies />} />
        <Route path="/supply/:id" element={<SupplyForm />} />
        <Route path="/supply-orders" element={<SupplyOrders />} />
        <Route path="/supply-order/:id" element={<SupplyOrderForm />} />
        <Route path="/writeoffs" element={<Writeoffs />} />
        <Route path="/writeoff/new" element={<WriteoffForm />} />
        <Route path="/revisions" element={<Revisions />} />
        <Route path="/revision/:id" element={<RevisionForm />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </div>
  );
}
