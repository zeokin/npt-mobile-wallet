export default function NeptuneText({ size = 160, color="black" }: { size?: number, color?:string}) {
  return (
    <img
      src={color=="black" ? "/npt-text-black.svg":"/npt-text-white.svg"}
      alt="Neptune"
      width={size}
      height={size}
      draggable={false}
    />
  );
}
