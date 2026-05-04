/*
 * ifaddr6 - FreeBSD IPv6 address discovery
 *
 * Uses getifaddrs() to enumerate interfaces and
 * ioctl(SIOCGIFALIFETIME_IN6) + ioctl(SIOCGIFAFLAG_IN6)
 * to query lifetimes and flags.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/ioctl.h>
#include <net/if.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <ifaddrs.h>
#include <time.h>
#include <errno.h>

#if defined(__FreeBSD__)
#include <netinet6/in6_var.h>
#endif

#ifndef ND6_INFINITE_LIFETIME
#define ND6_INFINITE_LIFETIME 0xffffffffU
#endif

#ifndef INET6_ADDRSTRLEN
#define INET6_ADDRSTRLEN 46
#endif

#ifndef IN6_IFF_TEMPORARY
#define IN6_IFF_TEMPORARY  0x0020
#endif

#ifndef IN6_IFF_CLATED
#define IN6_IFF_CLATED  0x0080
#endif

typedef struct {
    char addr[INET6_ADDRSTRLEN];
    char iface[IFNAMSIZ];
    unsigned int preferred_lft;
    unsigned int valid_lft;
    unsigned char is_temporary;
} ifaddr6_entry;

static void get_iface_addr_flags(int s, const char *ifname, const struct sockaddr_in6 *sin6,
                                  unsigned int *pltime, unsigned int *vltime, unsigned char *is_temporary) {
    time_t now = time(NULL);

#if defined(__FreeBSD__)
    struct in6_ifreq ifr6;

    /* Query lifetime */
    memset(&ifr6, 0, sizeof(ifr6));
    strncpy(ifr6.ifr_name, ifname, IFNAMSIZ - 1);
    ifr6.ifr_addr = *sin6;

    if (ioctl(s, SIOCGIFALIFETIME_IN6, &ifr6) == 0) {
        struct in6_addrlifetime lt = ifr6.ifr_ifru.ifru_lifetime;

        /*
         * FreeBSD in6_addrlifetime:
         * - ia6t_expire / ia6t_preferred: absolute expiration time (time_t)
         * - ia6t_vltime / ia6t_pltime: relative lifetime in seconds
         *
         * When ia6t_* absolute values are smaller than current time (or -1),
         * fall back to relative fields.
         */
        if (lt.ia6t_preferred != (time_t)-1 && lt.ia6t_preferred > now)
            *pltime = (unsigned int)(lt.ia6t_preferred - now);
        else if (lt.ia6t_pltime != (u_int32_t)-1)
            *pltime = lt.ia6t_pltime;

        if (lt.ia6t_expire != (time_t)-1 && lt.ia6t_expire > now)
            *vltime = (unsigned int)(lt.ia6t_expire - now);
        else if (lt.ia6t_vltime != (u_int32_t)-1)
            *vltime = lt.ia6t_vltime;
    }

    /* Query flags (must re-zero ifr6 after lifetime ioctl) */
    memset(&ifr6, 0, sizeof(ifr6));
    strncpy(ifr6.ifr_name, ifname, IFNAMSIZ - 1);
    ifr6.ifr_addr = *sin6;

    if (ioctl(s, SIOCGIFAFLAG_IN6, &ifr6) == 0) {
        /* IN6_IFF_CLATED indicates a cloned (temporary/privacy) address */
        *is_temporary = (ifr6.ifr_ifru.ifru_flags6 & (IN6_IFF_TEMPORARY | IN6_IFF_CLATED)) ? 1 : 0;
    }
#endif
}

int ifaddr6_query(const char *ifname, ifaddr6_entry *results, int max_results, int *error_code) {
    *error_code = 0;

    if (if_nametoindex(ifname) == 0) {
        *error_code = 1;
        return -1;
    }

    /* Create socket BEFORE getifaddrs - required on FreeBSD for ioctl to work */
    int s = socket(AF_INET6, SOCK_DGRAM, 0);
    if (s == -1) {
        *error_code = 2;
        return -1;
    }

    struct ifaddrs *ifap = NULL;
    if (getifaddrs(&ifap) == -1) {
        close(s);
        *error_code = 2;
        return -1;
    }

    int count = 0;

    for (struct ifaddrs *ifa = ifap; ifa != NULL; ifa = ifa->ifa_next) {
        if (ifa->ifa_addr == NULL ||
            ifa->ifa_addr->sa_family != AF_INET6) {
            continue;
        }
        if (strcmp(ifa->ifa_name, ifname) != 0)
            continue;

        struct sockaddr_in6 *sin6 = (struct sockaddr_in6 *)ifa->ifa_addr;
        struct in6_addr addr = sin6->sin6_addr;

        /* Skip link-local (fe80::/10) */
        if (addr.s6_addr[0] == 0xfe && (addr.s6_addr[1] & 0xc0) == 0x80)
            continue;

        /* Skip loopback (::1) */
        if (memcmp(addr.s6_addr, "\x00\x00\x00\x00\x00\x00\x00\x00"
                                  "\x00\x00\x00\x00\x00\x00\x00\x01", 16) == 0)
            continue;

        /* Skip ULA (fc00::/7) */
        if ((addr.s6_addr[0] & 0xfe) == 0xfc)
            continue;

        char addr_str[INET6_ADDRSTRLEN];
        if (inet_ntop(AF_INET6, &addr, addr_str, sizeof(addr_str)) == NULL)
            continue;

        unsigned int pltime = ND6_INFINITE_LIFETIME;
        unsigned int vltime = ND6_INFINITE_LIFETIME;
        unsigned char is_temp = 0;

        get_iface_addr_flags(s, ifname, sin6, &pltime, &vltime, &is_temp);

        if (count < max_results) {
            strncpy(results[count].addr, addr_str, INET6_ADDRSTRLEN - 1);
            results[count].addr[INET6_ADDRSTRLEN - 1] = '\0';
            strncpy(results[count].iface, ifname, IFNAMSIZ - 1);
            results[count].iface[IFNAMSIZ - 1] = '\0';
            results[count].preferred_lft = pltime;
            results[count].valid_lft = vltime;
            results[count].is_temporary = is_temp;
            count++;
        }
    }

    close(s);
    freeifaddrs(ifap);

    return count;
}
