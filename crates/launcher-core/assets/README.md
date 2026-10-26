# mcmod.buf

数据原始来源为 [MC 百科（mcmod.cn）](https://www.mcmod.cn)。

格式：gzip 压缩的 protobuf，`repeated Entry`（字段 1），每条 Entry：
1 = MC 百科 ID（int），2 = 中文名（「中文名 (English Name)」），3 = CurseForge slug，4 = Modrinth slug。
