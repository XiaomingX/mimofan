package blinded;

import java.util.Map;


























public class FastjsonCrossFile_A_Source {

    





    public static void handleRequest(String untrustedJson) {
        
        // 下一行即 trace 节点 A:28 —— 污点进入点
        Transport transport = new Transport();
        transport.setTypeName(untrustedJson);   // A:28 污点写入传输对象字段

        // 跨编译单元：将承载污点的 transport 交给文件 C 处理
        FastjsonCrossFile_C_Sink.process(transport);
    }
}
