package blinded;









public class ErrorBasedInfoLeakBy {

    static String handle(Exception e) {
        // 服务端记录细节，客户端仅见泛化信息
        System.err.println("internal error: " + e); // 仅服务端日志
        /*ANCHOR_1*/
        return "internal server error"; // 不泄露内部细节
    }
}
