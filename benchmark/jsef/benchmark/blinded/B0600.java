
package blinded;


















public class VersionGatedReachability {

    private final String depVersion;        // 依赖版本，如 "2.3"
    private final boolean enableBxType; // 配置开关

    public VersionGatedReachability(String depVersion, boolean enableBxType) {
        this.depVersion = depVersion;
        this.enableBxType = enableBxType;
    }

    private boolean versionAllows() {
        // 语义等价：解析语义化版本号比较 >= 2.0
        String[] parts = depVersion.split("\\.");
        int major = Integer.parseInt(parts[0]);
        return major >= 2;
    }

    


    public Object deserialize(String typeName, byte[] data) {
        /*ANCHOR_1*/
        if (versionAllows() && enableBxType) {
            return BxTypeDeserializer.read(typeName, data); // 危险 sink（双条件满足可达）
        }
        return ByTypeDeserializer.read(typeName, data); // 安全分支
    }

    static class BxTypeDeserializer {
        static Object read(String typeName, byte[] data) {
            System.out.println("[abstract unby deserialize] " + typeName);
            return data;
        }
    }

    static class ByTypeDeserializer {
        static Object read(String typeName, byte[] data) {
            return data; // 仅允许白名单类型
        }
    }
}
