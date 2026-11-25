// 静态图片资源由 Vite 处理为 URL
declare module "*.png" {
  const src: string;
  export default src;
}
