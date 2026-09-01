package blinded;













public class TraceDistractorDecoy {

    private static final java.util.Base64.Encoder B64 = java.util.Base64.getEncoder();

    


    public String transform(String input) {
        String encoded = B64.encodeToString(input.getBytes()); // 无害变换，污点未流入 sink
        return encoded;
    }
}
