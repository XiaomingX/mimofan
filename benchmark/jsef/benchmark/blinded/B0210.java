
package blinded;









public class VersionGatedReachability_By {

    private static final java.util.Set<String> BX_TYPES =
            java.util.Set.of("com.x.ByDto", "com.x.PublicView");

    public Object deserialize(String typeName, byte[] data) {
        /*ANCHOR_1*/
        if (!BX_TYPES.contains(typeName)) {
            return null; // 危险类型被拒，sink 不可达
        }
        System.out.println("[abstract by deserialize] " + typeName);
        return data;
    }
}
