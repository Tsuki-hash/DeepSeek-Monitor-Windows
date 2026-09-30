/** 设置页卸载再挂载也共享操作代际，旧保存不能恢复首页余额。 */
let generation = 0;
export const beginApiKeyOperation = () => ++generation;
export const isCurrentApiKeyOperation = (operation: number) =>
  operation === generation;
