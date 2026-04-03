export default function NeptuneLogo({ size = 80 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 80 80"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <defs>
        <linearGradient id="npt-logo-grad" x1="0" y1="0" x2="80" y2="80" gradientUnits="userSpaceOnUse">
          <stop stopColor="#4ECDC4" />
          <stop offset="1" stopColor="#A7F0E4" />
        </linearGradient>
      </defs>
      <circle cx="40" cy="40" r="40" fill="url(#npt-logo-grad)" />
      {/* Stylized "N" mark */}
      <path
        d="M27 55V29C27 27.3 28.3 26 30 26C31.2 26 32.3 26.7 32.8 27.8L40 44L47.2 27.8C47.7 26.7 48.8 26 50 26C51.7 26 53 27.3 53 29V55"
        stroke="#1A1D26"
        strokeWidth="4.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />
    </svg>
  );
}
