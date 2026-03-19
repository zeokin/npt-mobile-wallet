import { useNavigate, useLocation } from "react-router-dom";
import { Wallet, Clock, Settings } from "lucide-react";

export default function NavBar() {
  const navigate = useNavigate();
  const location = useLocation();
  const tabs = [
    { path: "/wallet", icon: Wallet, label: "Wallet" },
    { path: "/history", icon: Clock, label: "History" },
    { path: "/settings", icon: Settings, label: "Settings" },
  ];
  return (
    <nav className="flex border-t border-[var(--npt-border)] bg-[var(--npt-dark)]">
      {tabs.map(({ path, icon: Icon, label }) => {
        const active = location.pathname === path;
        return (
          <button
            key={path}
            onClick={() => navigate(path)}
            className={`flex-1 flex flex-col items-center py-2 text-xs transition-colors ${
              active ? "text-[var(--npt-blue)]" : "text-[var(--npt-muted)]"
            }`}
          >
            <Icon size={20} />
            <span className="mt-1">{label}</span>
          </button>
        );
      })}
    </nav>
  );
}
