package blinded;

import java.util.Map;

























public class FastjsonCrossFilePerturbed {

    


    static class Carrier {
        private String clz;

        
        void setClz(String clz) {
            this.clz = clz;   // :41 污点写入字段
        }

        
        String getClz() {
            return this.clz;   // :46 污点流出字段
        }
    }

    




    public static void handle(String rawPayload) {
        
        Carrier carrier = new Carrier();
        carrier.setClz(rawPayload);   // :58 污点写入传输对象字段

        // 单文件内：直接将承载污点的 carrier 送入 sink 处理
        process(carrier);
    }

    



    public static void process(Carrier carrier) {
        String clz = carrier.getClz();   // :69 从字段取回污点

        /*ANCHOR_1*/
        Object instance = build(clz);   // :72 sink：按不可信类名实例化
    }

    



    private static Object build(String clz) {
        // 模拟：autotype 开启时按 clz 实例化任意类
        System.out.println("[demo-only] instantiating type: " + clz);
        return new Object();
    }
}
