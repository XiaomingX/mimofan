package blinded;























public class FastjsonCrossFile_C_Sink {

    


    public static void process(FastjsonCrossFile_B_Transport transport) {
        String typeName = transport.getTypeName();   // C:21 从跨文件字段取回污点

        /*ANCHOR_1*/
        Object instance = instantiate(typeName);   // C:30 sink：按不可信类型名实例化
    }

    



    private static Object instantiate(String typeName) {
        // 模拟：autotype 开启时按 typeName 实例化任意类
        System.out.println("[demo-only] instantiating type: " + typeName);
        return new Object();
    }
}
