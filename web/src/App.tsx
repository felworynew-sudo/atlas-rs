import { Navigate, Route, Routes } from 'react-router-dom';
import Home from './pages/Home';
import Login from './pages/Login';
import NewSale from './pages/NewSale';
import ProductEdit from './pages/ProductEdit';
import Products from './pages/Products';
import SaleDetail from './pages/SaleDetail';
import Checklist from './pages/Checklist';
import ClientDetail from './pages/ClientDetail';
import Clients from './pages/Clients';
import ProductStats from './pages/ProductStats';
import Settings from './pages/Settings';
import ProfitStats from './pages/ProfitStats';
import Reports from './pages/Reports';
import Salary from './pages/Salary';
import ShiftPage from './pages/ShiftPage';
import RevisionForm from './pages/RevisionForm';
import RevisionStats from './pages/RevisionStats';
import SalesStats from './pages/SalesStats';
import SpendingForm from './pages/SpendingForm';
import Spendings from './pages/Spendings';
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
        <Route path="/shift" element={<ShiftPage />} />
        <Route path="/salary" element={<Salary />} />
        <Route path="/checklist" element={<Checklist />} />
        <Route path="/clients" element={<Clients />} />
        <Route path="/client/:id" element={<ClientDetail />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="/settings/:section" element={<Settings />} />
        <Route path="/spendings" element={<Spendings />} />
        <Route path="/spending/new" element={<SpendingForm />} />
        <Route path="/reports" element={<Reports />} />
        <Route path="/reports/sales" element={<SalesStats />} />
        <Route path="/reports/profit" element={<ProfitStats />} />
        <Route path="/reports/products" element={<ProductStats />} />
        <Route path="/reports/revisions" element={<RevisionStats />} />
        <Route path="/revisions" element={<Revisions />} />
        <Route path="/revision/:id" element={<RevisionForm />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </div>
  );
}
