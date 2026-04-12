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
    <nav className="flex bg-[var(--npt-bg)] border-t border-[var(--npt-border)] safe-bottom">
      {tabs.map(({ path, icon: Icon, label }) => {
        const active = location.pathname === path;
        return (
          <button
            key={path}
            onClick={() => navigate(path)}
            className={`flex-1 flex flex-col items-center pt-2 pb-1 text-[11px] font-medium transition-colors ${
              active ? "text-[var(--npt-blue)]" : "text-[var(--npt-muted)]"
            }`}
          >
            <Icon size={22} strokeWidth={active ? 2.2 : 1.8} />
            <span className="mt-0.5">{label}</span>
          </button>
        );
      })}
    </nav>
  );
}
