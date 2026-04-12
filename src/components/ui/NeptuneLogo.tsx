export default function NeptuneLogo({ size = 80 }: { size?: number }) {
  return (
    <img
      src="/npt-logo.svg"
      alt="Neptune"
      width={size}
      height={size}
      draggable={false}
    />
  );
}
