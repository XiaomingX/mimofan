
package blinded;









public class ActuatorExposure {

    



    static String dangerousConfig() {
        /*ANCHOR_1*/
        return "management.endpoints.web.exposure.include=*"; // 含 env, heapdump，无 spring-security
    }
}
